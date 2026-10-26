#!/usr/bin/env python3
"""Check the REAS documentation profile for this repository.

Adapted from the REAS checker (https://github.com/abenamer/reas, MIT). The
OKF bundle of this project is `docs/`. This is not an STE language checker.

Parse CommonMark links, images, headings, and HTML href/src/id attributes.
Resolve heading fragments with GitHub-style slugs. Do not fetch external URLs.
Check rule declarations in ATX or Setext headings throughout the repository.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import date, datetime
from html.parser import HTMLParser
from pathlib import Path
import re
import unicodedata
from urllib.parse import unquote, urlsplit

from markdown_it import MarkdownIt
import yaml

RULE = re.compile(r"^(REAS-[A-Z]+-\d{3})(?=$|[\s:])")
ACTOR = re.compile(r"^(?:human:[^\s]+|process:[^\s]+|[^\s/]+/[^\s/]+)$")
MARKDOWN = MarkdownIt("commonmark")


class UniqueKeyLoader(yaml.SafeLoader):
    """Reject duplicate YAML keys instead of silently using the last value."""


def unique_mapping(loader, node, deep=False):
    loader.flatten_mapping(node)
    result = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in result:
            raise yaml.constructor.ConstructorError(
                None, None, f"Duplicate YAML key: {key}", key_node.start_mark
            )
        result[key] = loader.construct_object(value_node, deep=deep)
    return result


UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, unique_mapping
)


def split_frontmatter(text: str):
    if not text.startswith("---\n"):
        return None, text, None
    match = re.search(r"\n---[ \t]*(?:\n|$)", text[4:])
    if match is None:
        return None, text, "Unclosed YAML frontmatter"
    raw = text[4:][:match.start()]
    body = text[4:][match.end():]
    try:
        metadata = yaml.load(raw, Loader=UniqueKeyLoader)
    except (yaml.YAMLError, TypeError) as exc:
        return None, text, f"Invalid YAML: {exc}"
    if not isinstance(metadata, dict):
        return None, text, "Frontmatter must be a mapping"
    return metadata, body, None


class HTMLReferences(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.anchors = set()

    def handle_starttag(self, tag, attrs):
        for name, value in attrs:
            if value is None:
                continue
            if name in {"href", "src"}:
                self.links.append(value)
            if name == "id" or (tag == "a" and name == "name"):
                self.anchors.add(value)

    handle_startendtag = handle_starttag


@dataclass
class Document:
    body: str
    headings: list[tuple[int, str]]
    links: list[str]
    anchors: set[str]


def parse_document(body: str) -> Document:
    tokens = MARKDOWN.parse(body)
    headings = []
    links = []
    anchors = set()
    used_slugs = set()
    html = HTMLReferences()

    def read_inline(children):
        for child in children or []:
            if child.type == "link_open":
                links.append(child.attrGet("href") or "")
            elif child.type == "image":
                links.append(child.attrGet("src") or "")
            elif child.type == "html_inline":
                html.feed(child.content)
            read_inline(child.children)

    for index, token in enumerate(tokens):
        if token.type == "heading_open":
            inline = tokens[index + 1]
            text = "".join(
                child.content for child in inline.children or []
                if child.type in {"text", "code_inline", "image"}
            )
            headings.append((int(token.tag[1:]), text))
            base = "".join(
                char for char in text.lower()
                if char in "-_ " or unicodedata.category(char)[0] not in "PSZC"
            ).replace(" ", "-")
            slug = base
            suffix = 0
            while slug in used_slugs:
                suffix += 1
                slug = f"{base}-{suffix}"
            used_slugs.add(slug)
            anchors.add(slug)
        elif token.type == "html_block":
            html.feed(token.content)
        if token.type == "inline":
            read_inline(token.children)
    links.extend(html.links)
    anchors.update(html.anchors)
    return Document(body, headings, links, anchors)


def valid_timestamp(value) -> bool:
    if isinstance(value, str):
        if "T" not in value:
            return False
        try:
            value = datetime.fromisoformat(value.replace("Z", "+00:00"))
        except ValueError:
            return False
    return isinstance(value, datetime) and value.utcoffset() is not None


def metadata_errors(meta: dict) -> list[str]:
    errors = []
    events = []
    if "generated" in meta:
        events.append(("generated", meta["generated"]))
    verified = meta.get("verified", [])
    if isinstance(verified, dict):
        verified = [verified]
    if not isinstance(verified, list):
        errors.append("verified must be a mapping or a list")
    else:
        events.extend(("verified", event) for event in verified)
    for field, event in events:
        if not isinstance(event, dict):
            errors.append(f"{field} must be a mapping")
            continue
        actor = event.get("by")
        if not isinstance(actor, str) or not ACTOR.fullmatch(actor):
            errors.append(f"Invalid {field}.by")
        if not valid_timestamp(event.get("at")):
            errors.append(f"Invalid {field}.at: use a datetime with a UTC offset")
    sources = meta.get("sources", [])
    if not isinstance(sources, list) or any(
        not isinstance(source, dict)
        or not isinstance(source.get("resource"), str)
        or not source["resource"].strip()
        for source in sources
    ):
        errors.append("Sources must include a resource")
    return errors


DEFAULT_BUNDLE = "docs"


def validate(root: Path, bundle_name: str = DEFAULT_BUNDLE) -> list[str]:
    root = root.resolve()
    bundle = root / bundle_name
    if not bundle.is_dir():
        return [f"The {bundle_name}/ OKF bundle is missing"]
    errors = []
    documents = {}
    concepts = set()
    seen = {}
    graph = {}
    for path in sorted(root.rglob("*.md")):
        if any(part in {".git", "target", ".venv"} for part in path.relative_to(root).parts):
            continue
        rel = path.relative_to(root).as_posix()
        try:
            content = path.read_text(encoding="utf-8")
        except (UnicodeError, OSError) as exc:
            errors.append(f"{rel}: Cannot read: {exc}")
            continue
        meta, body, err = split_frontmatter(content)
        if err:
            errors.append(f"{rel}: {err}")
        document = parse_document(body)
        documents[path] = document
        graph[path] = set()
        if path.is_relative_to(bundle):
            if path.name == "index.md":
                if path.parent == bundle:
                    if meta is None or set(meta) != {"okf_version"} or str(meta.get("okf_version")) != "0.2":
                        errors.append(f"{rel}: Bundle index must contain only okf_version: '0.2'")
                elif meta is not None:
                    errors.append(f"{rel}: Nested index must not contain frontmatter")
            elif path.name == "log.md":
                if meta is not None:
                    errors.append(f"{rel}: Log must not contain frontmatter")
                for level, heading in document.headings:
                    if level == 2:
                        try:
                            if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", heading):
                                raise ValueError
                            date.fromisoformat(heading)
                        except ValueError:
                            errors.append(f"{rel}: Log date must use YYYY-MM-DD: {heading}")
            else:
                concepts.add(path)
                if not isinstance(meta, dict) or not isinstance(meta.get("type"), str) or not meta["type"].strip():
                    errors.append(f"{rel}: Concept requires a non-empty type")
                if meta:
                    errors.extend(f"{rel}: {error}" for error in metadata_errors(meta))
                if not body.strip():
                    errors.append(f"{rel}: Empty concept")
        for _, heading in document.headings:
            if not heading.startswith("REAS-"):
                continue
            match = RULE.match(heading)
            if match is None:
                errors.append(f"{rel}: Invalid rule declaration: {heading}")
                continue
            rule = match.group(1)
            if rule in seen:
                errors.append(f"{rel}: Duplicate {rule} also in {seen[rule]}")
            seen[rule] = rel
    for path, document in documents.items():
        rel = path.relative_to(root).as_posix()
        for uri in document.links:
            try:
                parsed = urlsplit(uri)
            except ValueError:
                errors.append(f"{rel}: Invalid link URI: {uri}")
                continue
            if parsed.scheme or parsed.netloc:
                continue
            target = unquote(parsed.path)
            if not target:
                actual = path
            elif target.startswith("/"):
                base = bundle if path.is_relative_to(bundle) else root
                actual = (base / target.lstrip("/")).resolve()
            else:
                actual = (path.parent / target).resolve()
            if not actual.is_relative_to(root):
                errors.append(f"{rel}: Link leaves repository: {uri}")
                continue
            if not actual.exists():
                errors.append(f"{rel}: Broken local link: {uri}")
                continue
            if actual.is_dir():
                actual = next(
                    (candidate for name in ("index.md", "README.md")
                     if (candidate := actual / name) in documents), actual
                )
            if actual in documents:
                graph[path].add(actual)
            if parsed.fragment:
                fragment = unquote(parsed.fragment)
                if actual not in documents or fragment not in documents[actual].anchors:
                    errors.append(f"{rel}: Broken local fragment: {uri}")
    index = bundle / "index.md"
    if not index.is_file():
        errors.append(f"{bundle_name}/index.md: Bundle index is missing")
    else:
        reachable = set()
        pending = [index]
        while pending:
            path = pending.pop()
            if path in reachable or not path.is_relative_to(bundle):
                continue
            reachable.add(path)
            pending.extend(graph.get(path, set()) - reachable)
        for path in sorted(concepts - reachable):
            errors.append(
                f"{path.relative_to(root)}: Concept is not reachable from {bundle_name}/index.md"
            )
    return errors


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--bundle", default=DEFAULT_BUNDLE, help="bundle directory under root")
    args = parser.parse_args()
    errors = validate(args.root, args.bundle)
    for error in errors:
        print("ERROR:", error)
    if errors:
        print(f"FAIL: {len(errors)} errors")
        return 1
    print("PASS: OKF frontmatter profile, index reachability, rule declarations, local links and fragments")
    print("NOT CHECKED: full OKF conformance, STE language, external URLs, technical accuracy, historical ID reuse")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

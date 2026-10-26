---
type: Technical Reference
title: Documentation checker
description: What scripts/check_docs.py validates and what it does not.
tags: [documentation, validation]
---

# Documentation checker

`scripts/check_docs.py` is adapted from the
[REAS checker](https://github.com/abenamer/reas/blob/main/standard/checker.md)
under the MIT license. The bundle directory is `docs/`.

Run it from the repository root:

```bash
python -m pip install -r requirements-docs.txt
python scripts/check_docs.py
python -m unittest discover -s tests -v
```

## Checks

| Area | Check |
| --- | --- |
| Concept frontmatter | YAML mapping with unique keys and a non-empty `type` |
| Bundle index | `docs/index.md` with only `okf_version: "0.2"` |
| Navigation | Each concept is reachable from the index through bundle links |
| Rule declarations | A heading that starts with `REAS-` has a valid, unique identifier |
| Local links | Referenced files, directories, and heading fragments exist |
| Event metadata | `generated` and `verified` events have an actor and a UTC-offset time |
| Source metadata | Each `sources` entry has a `resource` |

## Differences from the REAS checker

- The bundle is `docs/`, selectable with `--bundle`.
- A repository without `REAS-` rule headings passes. This project cites
  REAS rules; it does not declare them.

## Limits

The checker does not fetch external URLs, does not validate STE language,
and does not establish technical accuracy.

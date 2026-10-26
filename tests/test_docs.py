"""Behavioral regression tests for the documentation checker (adapted from REAS)."""
from pathlib import Path
import tempfile
import unittest

from scripts.check_docs import split_frontmatter, validate


class CheckDocsTests(unittest.TestCase):
    def fixture(self, files=None):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        contents = {
            'docs/index.md': '---\nokf_version: "0.2"\n---\n# Index\n[Rule](rule.md)\n',
            'docs/rule.md': '---\ntype: Rule\n---\n# REAS-RUST-001: Rule\n',
        }
        contents.update(files or {})
        for name, content in contents.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding='utf-8')
        return root

    def append(self, root, content, name='docs/rule.md'):
        path = root / name
        path.write_text(path.read_text() + content)

    def test_valid_frontmatter(self):
        metadata, body, error = split_frontmatter('---\ntype: Rule\n---\nText')
        self.assertIsNone(error)
        self.assertEqual(metadata['type'], 'Rule')
        self.assertEqual(body, 'Text')

    def test_missing_frontmatter_is_reported_for_a_concept(self):
        root = self.fixture({'docs/rule.md': '# REAS-RUST-001\n'})
        self.assertTrue(any('Concept requires' in error for error in validate(root)))

    def test_invalid_yaml(self):
        _, _, error = split_frontmatter('---\ntype: [unclosed\n---\nBody')
        self.assertIsNotNone(error)

    def test_duplicate_yaml_keys_are_rejected(self):
        _, _, error = split_frontmatter('---\ntype: Rule\ntype: Other\n---\nBody')
        self.assertIn('Duplicate YAML key', error)

    def test_frontmatter_must_be_a_mapping(self):
        _, _, error = split_frontmatter('---\n- Rule\n---\nBody')
        self.assertEqual(error, 'Frontmatter must be a mapping')

    def test_unclosed_frontmatter(self):
        _, _, error = split_frontmatter('---\ntype: Rule\nBody')
        self.assertEqual(error, 'Unclosed YAML frontmatter')

    def test_code_does_not_declare_links_or_rules(self):
        root = self.fixture()
        self.append(root, '\n```md\n# REAS-RUST-001\n[bad](absent.md)\n```\n'
                    '\n    [bad](absent.md)\n\n`[bad](absent.md)`\n')
        self.assertEqual(validate(root), [])

    def test_real_repository(self):
        self.assertEqual(validate(Path(__file__).resolve().parents[1]), [])

    def test_missing_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(validate(Path(directory)), ['The docs/ OKF bundle is missing'])

    def test_missing_index(self):
        root = self.fixture()
        (root / 'docs/index.md').unlink()
        self.assertEqual(validate(root), ['docs/index.md: Bundle index is missing'])

    def test_wrong_index_version(self):
        root = self.fixture()
        path = root / 'docs/index.md'
        path.write_text(path.read_text().replace('0.2', '0.1'))
        self.assertTrue(any('Bundle index must' in error for error in validate(root)))

    def test_unindexed_concept_is_rejected(self):
        root = self.fixture({'docs/hidden.md': '---\ntype: Rule\n---\nHidden.\n'})
        self.assertTrue(any('not reachable' in error for error in validate(root)))

    def test_indirect_index_navigation_is_valid(self):
        root = self.fixture({'docs/deep/concept.md': '---\ntype: Reference\n---\nText.\n'})
        self.append(root, '\n[More](deep/concept.md)\n')
        self.assertEqual(validate(root), [])

    def test_navigation_cannot_leave_and_reenter_the_bundle(self):
        root = self.fixture({
            'README.md': '[Hidden](docs/hidden.md)\n',
            'docs/hidden.md': '---\ntype: Rule\n---\nHidden.\n',
        })
        self.append(root, '\n[Outside](../README.md)\n')
        self.assertTrue(any('hidden.md: Concept is not reachable' in error for error in validate(root)))

    def test_broken_inline_reference_and_image_paths(self):
        cases = ('[missing](absent.md)', '[missing][target]\n\n[target]: absent.md',
                 '![missing](absent.png)', '[target][]\n\n[target]: absent.md',
                 '[target]\n\n[target]: absent.md')
        for content in cases:
            with self.subTest(content=content):
                root = self.fixture()
                self.append(root, '\n' + content + '\n')
                self.assertTrue(any('Broken local link' in error for error in validate(root)))

    def test_reference_link_title_and_parentheses(self):
        root = self.fixture({'docs/file(one).md': '---\ntype: Reference\n---\nText.\n'})
        self.append(root, '\n[more][ref]\n\n[ref]: <file(one).md> "A title"\n')
        self.assertEqual(validate(root), [])

    def test_same_and_cross_file_fragments(self):
        for destination in ('#absent', 'index.md#absent'):
            with self.subTest(destination=destination):
                root = self.fixture()
                self.append(root, f'\n[bad]({destination})\n')
                self.assertTrue(any('Broken local fragment' in error for error in validate(root)))

    def test_github_style_heading_fragments(self):
        root = self.fixture()
        self.append(root, '\n## A `code` heading!\n\n## A `code` heading!\n'
                    '\n[one](#a-code-heading) [two](#a-code-heading-1)\n')
        self.assertEqual(validate(root), [])

    def test_unicode_and_explicit_html_anchors(self):
        root = self.fixture()
        self.append(root, '\n## Café\n\n<a id="stable"></a>\n'
                    '\n[one](#caf%C3%A9) <a href="#stable">two</a>\n')
        self.assertEqual(validate(root), [])

    def test_broken_html_links_are_rejected(self):
        root = self.fixture()
        self.append(root, '\n<a href="absent.md">bad</a>\n')
        self.assertTrue(any('Broken local link' in error for error in validate(root)))

    def test_absolute_paths_use_the_document_scope(self):
        root = self.fixture({'README.md': '[index](/docs/index.md#index)\n'})
        self.append(root, '\n[self](/rule.md#reas-rust-001-rule)\n')
        self.assertEqual(validate(root), [])

    def test_directory_navigation_uses_an_index(self):
        root = self.fixture({
            'docs/deep/index.md': '# More\n[Concept](concept.md)\n',
            'docs/deep/concept.md': '---\ntype: Reference\n---\nText.\n',
        })
        self.append(root, '\n[More](deep/)\n')
        self.assertEqual(validate(root), [])

    def test_directory_fragment_uses_readme_when_index_is_absent(self):
        root = self.fixture({'example/README.md': '# Example\n'})
        self.append(root, '\n[example](../example/#example)\n')
        self.assertEqual(validate(root), [])

    def test_link_cannot_escape_repository(self):
        root = self.fixture()
        self.append(root, '\n[escape](../../outside.md)\n')
        self.assertTrue(any('Link leaves repository' in error for error in validate(root)))

    def test_invalid_html_uri_reports_an_error_instead_of_crashing(self):
        root = self.fixture()
        self.append(root, '\n<a href="http://[">bad</a>\n')
        self.assertTrue(any('Invalid link URI' in error for error in validate(root)))

    def test_external_urls_are_not_fetched(self):
        root = self.fixture()
        self.append(root, '\n[external](https://invalid.example/missing#absent)\n')
        self.assertEqual(validate(root), [])

    def test_duplicate_rule_outside_bundle_is_rejected(self):
        root = self.fixture({'README.md': '# REAS-RUST-001: Another rule\n'})
        self.assertTrue(any('Duplicate REAS-RUST-001' in error for error in validate(root)))

    def test_setext_rule_declarations_are_checked(self):
        root = self.fixture({'README.md': 'REAS-RUST-001: Duplicate\n========================\n'})
        self.assertTrue(any('Duplicate REAS-RUST-001' in error for error in validate(root)))

    def test_malformed_rule_ids_are_rejected(self):
        for identifier in ('REAS-RUST-02', 'REAS-RUST-0010', 'REAS-rust-002'):
            with self.subTest(identifier=identifier):
                root = self.fixture()
                self.append(root, f'\n## {identifier}: Bad\n')
                self.assertTrue(any('Invalid rule declaration' in error for error in validate(root)))

    def test_log_is_not_a_concept(self):
        root = self.fixture({'docs/log.md': '# Updates\n\n## 2026-10-23\nA change.\n'})
        self.assertEqual(validate(root), [])

    def test_invalid_log_dates_are_rejected(self):
        root = self.fixture({'docs/log.md': '# Updates\n\n## 2026-02-30\nA change.\n'})
        self.assertTrue(any('Log date must' in error for error in validate(root)))

    def test_nested_index_frontmatter_is_rejected(self):
        root = self.fixture({'docs/deep/index.md': '---\nokf_version: "0.2"\n---\n# More\n'})
        self.assertTrue(any('Nested index must not' in error for error in validate(root)))

    def test_event_metadata_accepts_mapping_list_and_utc_offsets(self):
        for field in ('generated: {by: process:ci, at: 2026-10-23T12:00:00Z}',
                      'verified: {by: human:reviewer, at: "2026-10-23T08:00:00-04:00"}',
                      'verified: [{by: tool/1, at: 2026-10-23T12:00:00Z}]'):
            with self.subTest(field=field):
                root = self.fixture({'docs/rule.md': f'---\ntype: Rule\n{field}\n---\n# REAS-RUST-001\n'})
                self.assertEqual(validate(root), [])

    def test_invalid_event_actors_and_timestamps_are_rejected(self):
        cases = ('generated: {by: unknown, at: 2026-10-23T12:00:00Z}',
                 'verified: {by: human:reviewer}',
                 'verified: {by: human:reviewer, at: rubbish}',
                 'verified: {by: human:reviewer, at: 2026-10-23}',
                 'verified: {by: human:reviewer, at: "2026-10-23T12:00:00"}')
        for field in cases:
            with self.subTest(field=field):
                root = self.fixture({'docs/rule.md': f'---\ntype: Rule\n{field}\n---\n# REAS-RUST-001\n'})
                self.assertTrue(any('Invalid ' in error for error in validate(root)))

    def test_source_requires_a_resource(self):
        root = self.fixture({'docs/rule.md': '---\ntype: Rule\nsources: [{title: Missing}]\n---\n# REAS-RUST-001\n'})
        self.assertTrue(any('Sources must include' in error for error in validate(root)))

    def test_unknown_metadata_is_preserved_as_an_extension(self):
        root = self.fixture({'docs/rule.md': '---\ntype: Rule\ncustom: value\n---\n# REAS-RUST-001\n'})
        self.assertEqual(validate(root), [])


if __name__ == '__main__':
    unittest.main()

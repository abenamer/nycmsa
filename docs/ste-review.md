---
type: Review Procedure
title: Simplified Technical English review
description: Scope and result of the language review.
tags: [documentation, ste, review]
---

# Simplified Technical English review

**Overall result: full ASD-STE100 Issue 9 compliance is not established.**

## Review record: 2026-10-26

Reviewer: Claude (AI coding agent), plain-language pass during authoring.

Scope: `README.md`, `CLAUDE.md`, and the documents in `docs/`.

What was done: sentences were kept short, one instruction per procedural
sentence, conditions placed before instructions, and the terms of the
[terminology table](terminology.md) used throughout.

What was not done: no check of every word against the Issue 9 dictionary,
no part-of-speech review, no technical-noun or technical-verb category
decisions. Rust doc comments were not reviewed for language.

## Procedure for the next review

1. Identify the document revision and the scope.
2. Use the official Issue 9 rules and dictionary.
3. Resolve each vocabulary occurrence by meaning and part of speech.
4. Correct the text without a change to its technical meaning.
5. Run `python scripts/check_docs.py` after the edits.
6. Record the reviewer, date, revision, findings, and exceptions here.

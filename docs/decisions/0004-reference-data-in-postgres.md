---
type: Architecture Decision
title: Reference data in PostgreSQL
description: Why the catalog, test results, programs, and defaults are stored as versioned snapshots in the database.
tags: [decision, persistence]
---

# ADR 0004 — Reference data in PostgreSQL

## Context

The first versions read four data files at startup: the MySchools
catalog (5 MB of JSON), the state test results, the hand-entered
`programs.json`, and the form defaults `student.json`. The two downloaded
files changed with every refresh and did not belong in git; the owner
asked for all of them to live in PostgreSQL next to the simulation runs.

## Decision

- Five store ports: `CatalogStore`, `TestResultsStore`, `ProgramStore`,
  `StudentDefaultsStore`, and `ProfileStore` (added with the program
  profiles). `PostgresReferenceData` implements them all on the pool the
  run repository opens.
- Every save adds a snapshot row (`catalog_snapshots`, `score_snapshots`,
  `program_sets`, `student_defaults`) and its detail rows. The server
  reads the newest snapshot of each set at startup. Older snapshots stay,
  so a refresh never loses the previous figures.
- Catalog schools and programs are stored as the JSON documents the
  sources produce (`JSONB`), validated on read through the same adapter
  code that parsed the files. Test results are relational rows, since the
  app defines their shape.
- `nycmsa bootstrap` fills an empty database from the seed files and the
  two public sources; Compose runs it before the app starts.
- The seed files `data/programs.json` and `data/student.json` stay in
  git: they are hand-maintained input, imported with `nycmsa import`.
  The downloaded snapshots are not tracked.
- `serve` requires `DATABASE_URL`. The in-memory run repository remains
  the test double for the HTTP tests, which read a small catalog fixture.

## Consequences

- A refresh is a database write, not a file change and a redeploy.
- Each save is one transaction; a failed download leaves the previous
  snapshot in place.
- The history tables grow with each refresh: about 800 JSON rows per
  catalog snapshot and 1,100 rows per score snapshot. Pruning is manual.
- The CLI `sim` and `scan` commands still read files, for offline use
  and parity with the Python scripts.

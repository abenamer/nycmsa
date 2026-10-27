---
type: Architecture Decision
title: No user input on the server
description: Why simulation results moved from PostgreSQL to the visitor's browser, and what the server may store.
tags: [decision, privacy, persistence]
---

# ADR 0005 — No user input on the server

## Context

The site is public. Its only input is about a child: a lottery-number
prefix, the family's district and program codes, and an expected test
placement. ADR 0002 stored every simulation run in PostgreSQL and listed
recent runs on the home page, which showed each visitor's answers to
every other visitor. The questions page also opened prefilled with the
sample student from `data/student.json`.

## Decision

- PostgreSQL holds reference data only: what the school system publishes
  (the MySchools catalog, state test results, program profiles) and the
  hand-entered programs. The migration `20261028000004_drop_runs.sql`
  drops the `runs` table.
- The server stores nothing a visitor types and keeps no session. A
  simulation request is computed and answered in the same response; the
  JSON report carries no identifier. There is no cookie.
- Earlier results, the chosen programs, and the other answers live in the
  visitor's browser under `localStorage` keys that start with `nycmsa.`.
  The result page hands the submitted answers back to the browser in a
  `data-answers` attribute; the home page lists the browser's results and
  can resubmit them. Every page's footer states this and offers one
  button that forgets everything.
- The questions page opens blank. Only the model assumptions have a
  default (the DOE's waitlist rule, last year's numbers as published,
  test placement unknown). The scan page likewise shows no result until
  the visitor gives a lottery prefix.
- No analytics or third-party script is loaded. The reverse proxy in the
  deployment keeps Caddy's default of no access log.

## Consequences

- `RunRepository`, `RunQueries`, the in-memory and PostgreSQL run stores,
  and the `/runs` endpoints are removed. `SimulateStudent` is a pure use
  case with no port, and the HTTP state is no longer generic.
- A visitor who clears their browser storage loses their results; there
  is no server copy to recover from. "Show again" recomputes rather than
  replays, so a result can differ by Monte Carlo noise.
- The `student_defaults` table and `data/student.json` remain for the
  file-based `sim` command and the `import` step; the web server no
  longer reads them.
- A database backup contains nothing personal, which simplifies where it
  may be kept.

## Alternatives

Keeping the runs but hiding them from other visitors would still leave
personal answers on a public server with no account to tie them to.
Server-side sessions would need a cookie and a store, both of which this
decision rules out. The browser already has the storage the feature
needs.

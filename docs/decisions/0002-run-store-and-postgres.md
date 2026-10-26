---
type: Architecture Decision
title: Run store port and PostgreSQL adapter
description: Async port representation, SQLx usage, storage shape, and index choice.
tags: [decision, persistence, sqlx]
---

# ADR 0002 — Run store port and PostgreSQL adapter

## Context

Runs must survive a container restart. REAS-DATA rules call for explicit
queries in a named adapter, versioned migrations, data in a named volume,
and a reproducible build. The HTTP handlers run on a multi-threaded Tokio
runtime and need `Send` futures.

## Decision

- `RunRepository` is an async port. The `async_trait` macro supplies the
  boxed, `Send` future representation (REAS-ARCH-006 asks for the macro
  name to be recorded). Native `async fn` in the trait was rejected
  because its futures carry no `Send` bound that generic handlers can use.
- `SimulateStudent` exposes `compute` (CPU) and `record` (I/O) so the HTTP
  adapter can run the CPU step in `spawn_blocking` and the I/O step on the
  executor.
- `PostgresRunRepository` uses runtime-checked `sqlx::query`, not the
  compile-time checked macros. The build therefore needs no database and
  no `.sqlx/` offline metadata (REAS-DATA-004). The integration test in
  `tests/postgres_runs.rs` runs the queries against a real server.
- Migrations live in `migrations/` and run at startup through
  `sqlx::migrate!`.
- The student and report are stored as JSONB in the shape the HTTP API
  serves. Reads convert them through the same validated DTO conversions,
  so a corrupt row is reported as `RepositoryError::Corrupt` rather than
  trusted.
- The only index is the primary key on `id`. It serves `find` by id and
  `recent` (`ORDER BY id DESC LIMIT n`). The expected write volume is a
  few runs per minute at most; an index on `created_at` would add write
  cost without a query that needs it.
- Credentials arrive through `DATABASE_URL` only.

## Consequences

- The in-memory store stays the default and the test double.
- Reports are documents, not relational rows. A query across runs by
  program would need a new migration and query; none exists today.
- A stored report from an older DTO shape fails validation on read. A
  migration must rewrite the JSON when the shape changes.

## Alternatives

A synchronous port with the `postgres` crate inside `spawn_blocking` was
considered. It would keep one `execute` method but adds a second runtime
model. SQLx matches the REAS persistence rules and the tooling they name.

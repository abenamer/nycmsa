---
type: Architecture
title: Architecture
description: Code path, ports, adapters, composition root, and runtime behavior.
tags: [architecture, rust, ports-and-adapters]
---

# Architecture

The crate follows the REAS ports-and-adapters layout
([REAS-ARCH-001](https://github.com/abenamer/reas/blob/main/standard/ports-and-adapters.md#reas-arch-001)).
The [decision record](decisions/0001-ports-and-adapters.md) gives the
reasons.

## Code path

For a simulation request through the web interface:

1. Inbound adapter: `adapters::http::simulate_form` or `simulate_json`
   parses the input into a `SimulationRequest`.
2. Application use case: `application::SimulateStudent::compute` runs the
   trials; `SimulateStudent::record` stamps the run and stores it.
3. Port trait: `application::RunRepository` and `application::Clock`.
4. Outbound adapter: `adapters::InMemoryRunRepository` or
   `adapters::PostgresRunRepository`, and `adapters::SystemClock`.

For the catalog scan, `adapters::http::scan_page` calls
`application::ScanCatalog::execute`, which calls the pure `domain::scan`.
The catalog is read once at startup from the database
(`application::LoadReferenceData` through the `CatalogStore` port,
implemented by `adapters::PostgresReferenceData`) and parsed by
`adapters::myschools::catalog_from_documents`.

For a refresh, `composition::run_fetch` calls
`application::RefreshCatalog`, which downloads through the
`CatalogDownloader` port (`adapters::MySchoolsApiClient`) and stores
through the `CatalogStore` port (`adapters::PostgresReferenceData`).
`RefreshTestResults` does the same with `TestResultsDownloader`
(`adapters::InfoHubClient` by default, `adapters::NycOpenDataClient` on
request) and `TestResultsStore`. `ImportSeeds`
stores the seed files through `ProgramStore` and `StudentDefaultsStore`.
See [ADR 0004](decisions/0004-reference-data-in-postgres.md).

At startup `composition::simulation_programs` derives the simulation's
programs from the catalog (`domain::programs_from_catalog`) and merges the
hand-entered programs on top.

## Modules

| Module | Responsibility |
| --- | --- |
| `domain` | Values, programs, student, simulation, catalog rules. No I/O. |
| `application` | Use cases and port traits. Depends on `domain` and `serde_json::Value` for the raw catalog snapshot. |
| `adapters::dto` | Serde types for the JSON files and the HTTP API, with validated conversions. |
| `adapters::json_files` | Loads `programs.json` and `student.json`. |
| `adapters::myschools` | Parses the MySchools catalog and its prose. |
| `adapters::myschools_api` | Downloads the catalog over HTTPS. |
| `adapters::infohub` | Downloads and reads the DOE InfoHub results workbooks. |
| `adapters::open_data` | Downloads state test results from NYC Open Data. |
| `adapters::test_results` | Reads and writes a results CSV for offline tools. |
| `adapters::postgres_reference` | PostgreSQL store of the catalog, results, programs, and defaults. |
| `adapters::in_memory_runs` | Bounded in-process run store. |
| `adapters::postgres_runs` | PostgreSQL run store through SQLx. |
| `adapters::http` | Axum routes, Askama pages, JSON API. |
| `adapters::cli` | Clap commands and text rendering. |
| `composition` | Composition root: one function per subcommand. |

## Dependency direction

Axum, Askama, Clap, SQLx, and reqwest appear only in `adapters` and
`composition`. The domain depends on `rand`, `rand_chacha`, `rand_distr`,
and `thiserror`.

## Dispatch

Use cases take adapter type parameters bounded by port traits
([REAS-ARCH-006](https://github.com/abenamer/reas/blob/main/standard/ports-and-adapters.md#reas-arch-006)).
`composition::run_serve` uses `PostgresRunRepository`; the HTTP tests
build the same router with `InMemoryRunRepository` through the generic
`build_state`, so both use static dispatch.
`RunRepository` uses the `async_trait` macro, which supplies boxed `Send`
futures; see [ADR 0002](decisions/0002-run-store-and-postgres.md).

## Async work and CPU work

The simulation is CPU work. The HTTP adapter runs
`SimulateStudent::compute` in `tokio::task::spawn_blocking` under a
`Semaphore` with one permit per CPU, then awaits
`SimulateStudent::record` on the executor
([REAS-RUST-007](https://github.com/abenamer/reas/blob/main/standard/core-rust.md#reas-rust-007)).
`Trials` caps one request at 5,000,000 trials.

A started simulation does not stop when its client disconnects; the trial
cap bounds its duration. On Ctrl-C the server stops accepting connections
and finishes in-flight requests. `main` then shuts the runtime down with a
30-second deadline; blocking tasks still running after that are abandoned
when the process exits.

The CLI `sim` and `scan` commands run on the main thread without a
runtime. `fetch` and `serve` run on a multi-threaded Tokio runtime.

## State

A run has no workflow states: a request either produces a report or an
error before anything is stored. No state machine was needed. Stored runs
are immutable.

## Errors

Every fallible function returns a typed error. The HTTP adapter maps
`HttpError` to a status code and a JSON body `{"error": "..."}`. The CLI
prints the error chain to stderr and exits with status 1.

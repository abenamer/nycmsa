---
type: Audit Report
title: Self-audit
description: Which checks ran, where, and against which revision.
tags: [quality, audit, evidence]
---

# Self-audit

Report a check as passed only when execution evidence shows that result
(REAS-TEST-002). This page separates passed, failed, and not run.

## Local evidence: 2026-10-26

Environment: Linux x86_64 cloud container, Rust 1.99.1 (`rustc 1.99.1
(58cf8f9c3 2026-10-13)`, selected by `rust-toolchain.toml`), Python
3.13.17, PostgreSQL 16.15 (local cluster), cargo-audit (advisory database
of 1378 advisories). The checks ran on the working tree before the first
commit, so this local result has no immutable source commit. The CI
workflow in `.github/workflows/ci.yml` repeats every check on each pushed
revision; use its runs as the evidence for committed code.

| Check | Outcome | Detail |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Passed | |
| `cargo check --workspace --all-targets --locked` | Passed | |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed | |
| `cargo test --workspace --locked` | Passed | 79 unit tests, 12 HTTP adapter tests through the real router; 1 test ignored (PostgreSQL) |
| `cargo test --locked --test postgres_runs -- --ignored` | Passed | Against PostgreSQL 16.15 with `NYCMSA_TEST_DATABASE_URL`; migrations applied twice, insert, find, recent, cleanup |
| `cargo audit` | Passed with one ignored advisory and one warning | RUSTSEC-2023-0071 (`rsa`) ignored in `.cargo/audit.toml`: the crate is locked but not compiled for this target. RUSTSEC-2026-0374 warns that `basic-toml` (a dependency of `askama_derive`) is unmaintained; no action yet. |
| `python scripts/check_docs.py` | Passed | |
| `python -m unittest discover -s tests` | Passed | 36 test methods |
| Server smoke test, in-memory store | Passed | `GET /healthz`, `GET /`, `POST /simulate` (303 to `/runs/1`), `GET /runs/1`, `GET /api/runs/1`, `GET /scan`, invalid JSON body gives 422 |
| Server smoke test, PostgreSQL store | Passed | Run stored through the API; server restarted; `GET /api/runs` listed the run; form submit stored a second run; `_sqlx_migrations` holds version 20261026000001; Ctrl-C logged `shutdown requested` and exited |
| Comparison with the Python original | Passed within Monte Carlo error | Same inputs, 200,000 trials, seed 0. Python: P(initial) 93.5%, P(any) 98.1%, Mark Twain Science 67.2%/9.3%. Rust: 93.6%, 98.1%, 67.3%/9.2%. Different generators, so the numbers are not expected to be equal. |
| Comparison of the catalog scan | Passed, identical | `nycmsa scan` text equals `scan_out.txt` from the Python project after whitespace trimming; the 59-row CSV equals `scan_d15_d20.csv` field by field. |
| `docker build` and `docker compose up` | Not run | No Docker daemon in the development container. The CI `docker` job builds the image and runs `sim` inside it. |
| `nycmsa fetch` against the live MySchools API | Not run | The development container has no route to `myschools.nyc`. The adapter compiles and follows the Python fetcher's paging; its live behavior is unverified. |
| Full ASD-STE100 language review | Not run | See the [STE review](ste-review.md). |
| Miri, sanitizers | Not applicable | The crate contains no `unsafe` code. |

## Local evidence: 2026-10-27 (parent-friendly form)

Same environment. The web form changed to chips and plain questions; the
results page gained the stage sections. The checks below ran on the
working tree before commit.

| Check | Outcome |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --workspace --all-targets --locked` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo test --workspace --locked` | Passed; see the counts in the commit message |
| `python scripts/check_docs.py` and the checker tests | Passed |

## Local evidence: 2026-10-27 (all districts, test scores, reference data in PostgreSQL)

Same environment, PostgreSQL 16.15 local cluster. The network policy
changed during the session: `myschools.nyc` and `data.cityofnewyork.us`
became reachable.

| Check | Outcome |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --workspace` | Passed: 94 unit tests, 13 HTTP adapter tests; 2 ignored (PostgreSQL) |
| `cargo test --test postgres_runs --test postgres_reference -- --ignored` | Passed against the local server: run round trip; catalog, scores, programs, and defaults snapshots round trip with the latest snapshot winning |
| `python scripts/check_docs.py` and the checker tests | Passed |
| `nycmsa bootstrap` against the live sources | Passed in 1 m 52 s: 9 hand-entered programs and the defaults stored; catalog snapshot of 477 schools from the MySchools API (all districts); test results for 1,124 schools from NYC Open Data (ELA 2023, Math 2023). A second run reported that every set was present. |
| `nycmsa serve` from that database | Passed: loaded the snapshots, built 662 simulation programs from 784 catalog programs, served the form, a simulation, and a District 21 scan with ELA and Math figures |
| `nycmsa serve` against an empty database | Failed as designed: `no catalog snapshot in the database; run nycmsa bootstrap first` |
| Comparison of the catalog scan with Python | No longer identical by design: borough priority groups now admit students; see the domain model |
| `docker compose up` with the `init` service | Not run: no Docker daemon in the development container. The CI `docker` job builds the image and runs `sim` inside it; the Compose flow is unverified. |

## Remote evidence

None yet. After the first push, record here the GitHub Actions run that
passed for the pushed commit, with its commit hash and the jobs it ran.

## Limits

- The in-memory adapter is a test double; the PostgreSQL behavior is
  established only by the integration test above.
- The comparison with Python covers one student configuration and one
  catalog snapshot (districts 15 and 20).
- The documentation checker validates structure, not technical accuracy.

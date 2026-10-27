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

## Local evidence: 2026-10-27 (InfoHub test results)

Same environment; `infohub.nyced.org` became reachable.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test` | Passed: 97 unit tests (3 on the fixture workbooks), 13 HTTP tests; 2 ignored |
| `nycmsa fetch-scores` (InfoHub) | Passed in 17 s: two workbooks of about 50 MB, 1,129 schools, ELA 2026 and Math 2026, stored as a new snapshot |
| Cross-check of the Rust reader | Passed: the same two workbooks read with Python `openpyxl` 3 (filter `Grade = All Grades`, `Category = All Students`, latest year, `s` as unknown) gave 1,129 schools and 0 differing fields against the stored snapshot. 1,122 schools have an ELA figure and 1,105 a Math figure. |
| Scan page | Passed: shows "2026 New York State tests" and the caveats section; M.S. 442 reads ELA 77.6%, Math 85.0% |

The cross-check is a one-time local result for the workbooks published on
2026-08-03. Repeat it after each refresh; the reader detects a missing
sheet or column but not a renamed value column.

## Local evidence: 2026-10-27 (program picker and profiles)

Same environment.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test` | Passed: 104 unit tests, 14 HTTP tests; 2 ignored |
| `cargo test --test postgres_reference --test postgres_runs -- --ignored` | Passed, including the profile snapshot round trip |
| `nycmsa fetch-profiles` against MySchools | Passed in 4 m 28 s: 477 school lookups, 784 program profiles stored; 782 have a school overview, 36 a program description |
| `nycmsa reclassify-profiles` | Passed: new snapshot in seconds |
| Tag distribution after reclassification | Zoned 236, Arts 216, STEM 112, Humanities 62, Dual language 59, Honors 39, Careers 6; 234 programs General only. The first rule set, which matched substrings and treated the overview like program text, had tagged 615 programs Arts and 553 Humanities; see the domain model for the fix. |
| Pages | Passed: `/pick` and `/profiles` render from the database; the picker's ranking reaches the questions page through `?ranking=` |
| Picker and form memory in the browser | Not run: no browser automation in this check. The JavaScript is small and hand-reviewed; a browser test is a gap. |

## Local evidence: 2026-10-27 (code review fixes on pull request 1)

Same environment. The ten findings from the inline code review on the
first pull request were applied in one change; see the pull request
thread for the list.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test` | Passed: 106 unit tests, 14 HTTP tests; 2 ignored |
| GitHub Actions `rust` job on the first push (c17bf97) | Failed: `the_bundled_results_file_loads` read `data/school_test_results.csv`, which this change removed from git. The test had passed locally only because the untracked file was still on disk. The test was removed (the CSV round-trip test covers the reader) and the suite was re-run locally with the file moved away. |
| GitHub Actions `rust` job on the second push (341e1f7) | Failed at `cargo audit`: five advisories against `quick-xml` 0.38 (RUSTSEC-2026-0194, -0195) and `zip` 4.6 (RUSTSEC-2026-0367, -0368, -0369), both reached only through `calamine` 0.31. Fixed by moving to `calamine` 0.37, which depends on `quick-xml` 0.42 and `zip` 9.1; no source change was needed. `cargo audit` is clean locally apart from the allowed `basic-toml` unmaintained warning. |
| Reader cross-check after the `calamine` upgrade | Passed: `nycmsa fetch-scores --source infohub` re-read the live workbooks in 16 s; 1,129 schools and 0 differing fields against the snapshot the previous `calamine` had produced. The three fixture-workbook tests also pass. |
| `cargo test --test postgres_reference --test postgres_runs -- --ignored` | Passed |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed: 36 doc tests |
| `nycmsa reclassify-profiles` on the stored 784 profiles | Passed. The snapshot source carries one `(reclassified)` suffix after two runs. Tags at or above the 0.5 threshold: Zoned 236, Arts 216, STEM 112, Dual language 59, Humanities 59 (was 62 before "language arts" and "social studies" stopped counting), Honors 39, Careers 6; 237 programs General only. |
| Refresh with a failing school | Covered by two unit tests on `RefreshProfiles`: one failed lookup is skipped, counted, and named in the snapshot source; when every lookup fails nothing is saved. The live path was not re-run. |

## Local evidence: 2026-10-27 (picker arrows and sorting)

Same environment. The picker's move-up and move-down buttons rendered
white on white because the page-wide `button` rule sets white text and
the picker rule cleared only the background; the fix sets the text color
explicitly. The picker table now shares the scan page's sort script
(`templates/sortable.html`).

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test` | Passed: 106 unit tests, 14 HTTP tests; 2 ignored |
| `python scripts/check_docs.py` | Passed |
| Browser check with Playwright (Chromium) against `nycmsa serve` on the local database, 662 programs | Passed: the move-up button's computed color is the ink color `rgb(27, 31, 36)` on a transparent background; moving the second chosen program up swaps the two and the order survives a reload; two clicks on "Seats" sort 662 rows descending (581, 392, 385, ...) with the header marked `desc`; one click on "ELA" sorts ascending from 8.5% to 97.9% with the blank cells last. The script is not checked in; it lives in the session's scratch directory. |
| Code review on pull request 2 | Five findings, all applied: the arrow styling is scoped to the move buttons so × keeps its own look; the sort script uses the column's `cellIndex`, computes each row's key once, and takes Enter or Space on a focused heading with `aria-sort`; one `ScoreCell { shown, raw }` type serves both the scan and picker rows. |
| Browser check after the review fixes | Passed: × is muted grey at 1.1rem and turns red on hover; the arrows are ink at 0.8rem; focusing "Seats" and pressing Enter twice sorts 662 rows descending with `aria-sort="descending"`, `tabindex="0"` and `role="button"`; Space on "ELA" sorts ascending with blanks last and clears the "Seats" marker; the scan page's "Seats" heading still sorts 642 rows. |

## Local evidence: 2026-10-27 (Droplet deployment files)

Same environment. No Rust code changed.

| Check | Outcome |
| --- | --- |
| `docker compose -f deploy/compose.yaml config` with `POSTGRES_PASSWORD` set | Passed; without it Compose stops with "set POSTGRES_PASSWORD in .env", as intended |
| `deploy/cloud-init.yaml` | Parses as YAML with the expected top-level keys. The cloud-init schema checker was not available here. |
| `bash -n deploy/update.sh deploy/backup.sh` | Passed |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| Creating a Droplet from the cloud-init file | Not run: no DigitalOcean credentials in this environment. The first real Droplet is the test; record its outcome here. |
| CI image push to `ghcr.io/abenamer/nycmsa` | Not run until the change merges: the step runs only on a push to `main`. Check the package exists and is public after the merge. |
| Code review on pull request 5 (the same change reopened on top of the privacy change) | Four findings, all applied: `update.sh` runs as one function read in full before it executes, so refreshing itself cannot garble the run; a failed file fetch is logged and skipped instead of aborting before the image pull; cloud-init tries `docker compose up -d` three times a minute apart, and every later `up -d` starts a stopped `init` again; the operations doc names the `publish` job. |
## Local evidence: 2026-10-28 (no user input on the server)

Same environment. Simulation runs are no longer stored; results live in
the browser (ADR 0005). The `runs` table, its stores, port, and
endpoints are removed; the questions and scan pages open blank; every
page carries the privacy note and a "forget everything" button.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test --locked` | Passed: 99 unit tests, 14 HTTP tests; 1 ignored |
| `cargo test --locked --test postgres_reference -- --ignored` | Passed against the local server through the shared `adapters::postgres` pool |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| Migration 4 on the local database | Applied by `serve`; `to_regclass('runs')` is null afterwards, four migrations recorded |
| Browser check with Playwright (Chromium) against `nycmsa serve` | Passed: `/` opens with an empty lottery field, empty ranking, no district or family chip checked; submitting the form renders the result at `/simulate` with status 200 and no `Set-Cookie`; the browser's `nycmsa.results` then holds one entry (programs, lottery prefix, chance, answers) and still one after a reload; the context has zero cookies; `/` lists that result and "Show again" recomputes it without adding a duplicate; "Forget everything this browser remembers" leaves no `nycmsa.` key and an empty form; `/scan` opens with an empty lottery field and no rows; `/profiles` without a district says to pick one. |
| HTTP tests | `/api/simulate` returns the report with no `id`; `/api/runs`, `/api/runs/1` and `/runs/1` return 404; the result page carries `data-answers` with the submitted fields. |
| Code review on pull request 4 | Eight findings, all applied: the scan page never substitutes a lottery prefix (the API keeps its documented default); one scan-page flow instead of three; `serve` no longer requires the seed student; "Show again" keeps the form's memory in step with the result shown; a result is forgotten by its timestamp, not a stale index; a stale doc comment and ADR 0001's port example fixed; the API reference states that no per-client rate limit exists. |
| Security review on pull request 4 | No findings. Checked: attribute escaping of `data-answers` (askama entity-escapes quotes and angle brackets, and every submitted field except free-form attributes is validated to alphanumerics or hex), the inline scripts use `textContent` and DOM APIs only, "Show again" can only post same-origin form fields that the server validates, no handler logs form fields, no cookie, no endpoint returns another visitor's data. |

## Local evidence: 2026-10-27 (favicon and the "about" notice)

Same environment. No Rust code changed.

| Check | Outcome |
| --- | --- |
| `cargo check --all-targets`, `cargo test --locked --test http_api` | Passed: the templates compile and the 14 HTTP tests pass |
| `python scripts/check_docs.py` | Passed |
| Favicon | An inline SVG data URI in `templates/base.html`, so no static file route is needed; the browser check confirms the `link rel="icon"` resolves to an SVG the page can load |
| "About" notice | On the home page and in every page's footer: not a DOE website, built by a parent for his own family, nothing official, link to the code |

## Local evidence: 2026-10-27 (lottery number wording)

Same environment. Wording only: the hint under the lottery question on
the questions and scan pages, the README, and the domain model now say
that the NYC Department of Education assigns the random number and that
MySchools is where a family sees it, with a link to the DOE's page on
random numbers in admissions. That page was fetched through the
environment's proxy and read; it states that each applicant is assigned
a random number once they begin an application, that middle school
applicants can view it in MySchools under "Edit Profile" after they
start the application, that it is a UUID sorted `0` to `9` then `A` to
`F`, and that applicants receive a new random number for each waitlist.

| Check | Outcome |
| --- | --- |
| `cargo check --all-targets`, `cargo test --locked --test http_api` | Passed |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
## Local evidence: 2026-10-27 (sort by distance)

Same environment. The catalog's school coordinates (the DOE's geocoded
addresses) now reach the picker and scan tables as `data-lat` and
`data-lon`; a "Where do you live?" box geocodes the typed address in the
browser with the city's GeoSearch service, keeps the coordinates in
`nycmsa.home`, fills a Distance column with straight-line miles, and
sorts by it. The server never sees the address.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test --locked` | Passed; the fixture test checks that every fixture school is geocoded and that M.S. 442 sits at 40.6562, -73.9826; a unit test rejects the `0, 0` placeholder, out-of-range and non-numeric values |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| GeoSearch from this environment | Not reachable: the environment's network policy denies `geosearch.planninglabs.nyc`. The browser calls it directly, so the first real use in a browser is the test of that call; the page shows a message when the lookup fails or finds nothing. |
| Browser check with Playwright (Chromium) | Passed with a stubbed geocoder response: typing an address fills the Distance column, sorts the picker ascending, survives a reload from `nycmsa.home` (sorted again), and "Forget" clears it. |
| Code review on pull request 8 | Nine findings, all applied: a wrong coordinate in this table; a geocoder error status now reads as a failed lookup, not "not found"; the picker's filter listener no longer fires on the address box; a reload sorts by distance again; a malformed stored home is ignored; the `0, 0` placeholder is not a location; doc comments on the right structs; the CSV's new columns are appended, not inserted; and why a bad coordinate is `None` rather than an error is stated on the function. |
| Security review on pull request 8 | No findings. Checked: the geocoder's label and coordinates reach the page only through `textContent`, `value` and arithmetic; the fetch URL encodes the address; the address box has no form name and no form action, so nothing carries it to this server; coordinates are numeric in the templates, JSON and CSV. |

## Local evidence: 2026-10-27 (map and directions links)

Same environment. Each school's street address (from the catalog) now
shows under its name in the picker and scan tables as a Google Maps
link by coordinates, with a Directions link that opens Google Maps
transit directions to the school and leaves the starting point blank.
The visitor's home never goes into a link. The scan JSON and CSV carry
`address`.

| Check | Outcome |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, `cargo test --locked` | Passed; the fixture test checks M.S. 442's exact address; unit tests cover the title-casing (ordinals, a state and ZIP anywhere, a repeated tail, empty parts) and both link builders, including the address-text fallback when a school has no coordinates; the picker test checks that the links carry the school's coordinates and no `origin` |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| Code review on pull request 9 | Seven findings, all applied: the state-and-ZIP rule keys on the part's shape and a repeated tail is kept once (one catalog address had one); doc comments on the right functions; `rel="noopener noreferrer"` on the links; a unit test for the address-text fallback; one `map_links.html` include instead of two copies; the fixture test binds the program once and asserts the exact address; `with_address` borrows. |
| Security review on pull request 9 | No findings. Checked: the address reaches the page only through askama's escaping and the link hosts are constants, so no attribute break-out, scheme injection or redirect; the default referrer policy sends only the origin to Google, and `noreferrer` now sends nothing; the CSV's free-text columns predate this change and come from the DOE; nothing reads the stored home. |

## Local evidence: 2026-10-27 (cloud-init on Debian)

Same environment. The first real Droplet was created with Debian 13
rather than Ubuntu 24.04; cloud-init ran its firewall steps and then
failed at Docker's repository, which the file named for Ubuntu with the
host's codename. The file now takes the distribution id and codename
from `/etc/os-release`, so either system gets the right repository.

| Check | Outcome |
| --- | --- |
| `deploy/cloud-init.yaml` | Parses as YAML; the repository line expands to `https://download.docker.com/linux/debian trixie stable` on a Debian 13 `/etc/os-release` and `.../linux/ubuntu noble stable` on an Ubuntu 24.04 one (checked with a shell on sample files) |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| The Droplet itself | Not re-run from this environment, which cannot reach it; the owner finishes the install over SSH with the same commands, recorded in the operations doc |
| Code review on pull request 10 (low) | No findings |
| Security review on pull request 10 | No findings. Checked: both new lines source the root-owned `/etc/os-release` that the vendor image ships, before any user or service can write to the disk; `$ID` expands inside a quoted URL path on `download.docker.com` over TLS, so it cannot change the host the key comes from; the apt line stays bound to that host and to `signed-by=docker.asc` |

## Local evidence: 2026-10-27 (home page heading)

Same environment, revision on branch `claude/home-heading` over `main`
b23da1e. The home page now opens with the heading "NYC Middle School
Admissions" and the subheading "What are the chances of your child
getting into the NYC middle school of your choice?"; the browser title
matches the heading. No Rust code changed outside the test.

| Check | Outcome |
| --- | --- |
| `cargo fmt --all`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test` | 102 unit tests and 14 HTTP tests passed; the blank-home test asserts the heading and subheading |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| Code review on pull request 11 (low) | No findings |
| Security review on pull request 11 | No findings. The change is static text in `home.html` with no template expression, no new input and no new link, plus a test assertion |

## Local evidence: 2026-10-27 (school demand history)

Same environment, branch `claude/infohub-demand` over `main` b9374e7.
A new reference set: the DOE's yearly reports on school applications,
offers and seats (Local Law 72 of 2018), grade 6, per school and
residential district, fall 2018 to fall 2025; a `/schools/{dbn}` page;
"Listed the school" and "Competed" columns on the scan and picker pages
and in the scan JSON and CSV; and the caveats and domain doc now state
the DOE's definition of "applicants" (children considered for a seat,
not everyone who listed the program).

| Check | Outcome |
| --- | --- |
| `cargo fmt --all`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test` | 112 unit tests (domain demand helpers, both workbook layouts from fixture files, the refresh use case skipping a failed year) and 17 HTTP tests (school page newest-first, 404 for an unknown DBN, the new columns and JSON fields) passed |
| `cargo test --test postgres_reference -- --ignored` against the local PostgreSQL 16 | Passed, with the demand round trip (two years, district rows, suppressed cells) |
| `nycmsa fetch-demand` against the real sources | Read all eight workbooks (six from InfoHub, fall 2020 and 2021 from NYC Open Data attachments) in 87 s; stored 511 schools, 3,827 school-years, 22,740 rows; peak resident memory 244 MB, so the 1 GB Droplet needs its swap during this step |
| Cross-check | M.S. 442 (15K442) fall 2025 on the school page: 115 seats, 1,585 listed, 564 competed, 138 offers; the same cells read from the workbook with openpyxl before the Rust reader existed. Residential rows sort by competing families: District 15 (930 listed, 358 competed, 97 offers), then 20, 17, 13 |
| Citywide totals read with openpyxl | Fall 2025: 69,248 seats, 177,502 listings, 73,637 considered, 51,200 offers; the DOE's 2024 release counts 56,206 applicants, so each child listed about three schools and competed at about 1.3 |
| `python scripts/check_docs.py`, `python -m unittest discover -s tests` | Passed |
| Not run | The Droplet's bootstrap with the new step (needs the merge); the InfoHub URLs for new years do not exist yet and must be added by hand |

## Remote evidence

| Revision | Run | Outcome |
| --- | --- | --- |
| adf9f22 (pull request 1, 2026-10-27) | GitHub Actions run 40453826962 (pull request) and 40453821113 (push) | Passed: `docs`, `rust` (fmt, check, clippy, tests, both PostgreSQL integration tests against the `postgres:17` service, `cargo audit`), `docker` (image build and `sim --trials 1000`) |
| 93a411c (pull request 2, 2026-10-27) | GitHub Actions run 40456654387 (pull request) and 40456650876 (push) | Passed: the same three jobs |
| 077fcc3 (pull request 4, 2026-10-27) | GitHub Actions run 40468033725 (pull request) and 40468028557 (push) | Passed: the same three jobs, with the PostgreSQL step running only the reference-store test |
| 6aa1a53 (pull request 8, 2026-10-27) | GitHub Actions run 40472544141 (pull request) and 40472539221 (push) | Passed: the same three jobs |
| de29950 (pull request 9, 2026-10-27) | GitHub Actions run 40479276795 (pull request) and 40479271672 (push) | Passed: the same three jobs |

The two earlier pushes on the same pull request failed in `rust`; the
local-evidence table above records why and the fix for each. A commit
that changes only this file is not recorded here.

## Limits

- The in-memory adapter is a test double; the PostgreSQL behavior is
  established only by the integration test above.
- The comparison with Python covers one student configuration and one
  catalog snapshot (districts 15 and 20).
- The documentation checker validates structure, not technical accuracy.

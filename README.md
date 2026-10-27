# nycmsa — NYC middle school admissions Monte Carlo

Not a NYC Department of Education website: a parent with a child
applying to middle school built this for his own family and opened it to
others. A Rust port of
[nyc-ms-admissions-sim](https://github.com/abenamer/nyc-ms-admissions-sim)
with a web interface. It estimates one student's chances across a ranked
list of NYC middle school programs from last year's MySchools "Seats and
Demand" figures, scans the MySchools catalog for every program the
student is eligible for, and shows each school's year-by-year demand from
the DOE's reports: families who listed it, families who competed for a
seat, and offers.

The code follows the [REAS](https://github.com/abenamer/reas) engineering
standard: explicit domain types, ports and adapters, one composition root,
typed errors, and an OKF documentation bundle in [`docs/`](docs/index.md).

## What the web page asks

- The first two digits of the random number New York City Public Schools
  assigned your child when you started an application, shown in MySchools
  under "Edit Profile" (the first two hex characters place the number to
  0.4%).
- How high your child will place on a screened test: don't know, or a
  quarter of the field.
- Optionally, where you live, to sort the program lists by straight-line
  distance. The browser geocodes the address with the city's GeoSearch
  service and keeps only the coordinates; the server never sees it.
- Which school district you live in (any of the 32), free or reduced-price
  lunch, and the programs where a sibling, zone, or continuing-student
  priority applies.
- The ranked list of program codes, chosen on a picker page with district,
  kind-of-program, and screened filters; the list and the answers are
  remembered in the browser until reset.
- Two assumption chips: whether waitlists draw a new number or keep the
  old one, and whether to take last year's numbers as published or assume
  schools reach further down their lists.

The result gives one chance of an offer per program, then walks through
the stages: the lottery round, screened programs, and waitlists. The
"Which schools?" page lists every eligible program citywide with last
year's offers for your priority group and the school's ELA and math
proficiency from NYC Open Data, sortable by any column. The JSON API and
the CLI expose every model input directly.

All figures are last year's admissions numbers as reported on the
MySchools website. The server derives programs for every district from
the full catalog; `data/programs.json` holds hand-checked figures that
replace the derived entries for the codes it covers. Every refresh adds a
snapshot to the database; older snapshots stay.

## Run

Everything the server reads lives in PostgreSQL: the MySchools catalog,
state test results, the hand-entered programs, and the program profiles.
Nothing a visitor types is stored there. Answers and results stay in the
visitor's browser.

```bash
export DATABASE_URL=postgres://nycmsa:secret@127.0.0.1/nycmsa
cargo run --release --locked -- bootstrap   # seeds, catalog, scores
cargo run --release --locked -- serve
# open http://127.0.0.1:8080
```

Command line, as in the Python original:

```bash
cargo run --release --locked -- sim --lottery B0 --twain-pct 0.25 --rank K187A K442S
cargo run --release --locked -- fetch          # new catalog snapshot
cargo run --release --locked -- fetch-scores   # new test-results snapshot
cargo run --release --locked -- scan tests/fixtures/myschools_d15_d20.json --lottery BD
```

With Docker Compose, a one-shot `init` service bootstraps the database on
the named volume `nycmsa_pgdata` and the app serves from it:

```bash
docker compose up --build
```

To host it, `deploy/` has a production Compose file and a cloud-init
file that set up a $6 DigitalOcean Droplet from the image CI publishes,
and CI can deploy each merge to that server over a single-purpose SSH key.
See [operations](docs/operations.md) for the steps, backups, restore,
migrations, and hosting, and the [API reference](docs/api.md) for the
JSON endpoints.

## Layout

| Path | Contents |
| --- | --- |
| `src/domain/` | Model and pure rules: lottery, programs, student, simulation, catalog scan |
| `src/application/` | Use cases and ports |
| `src/adapters/` | HTTP (Axum, Askama), CLI (Clap), JSON files, MySchools parser and client, PostgreSQL reference store |
| `src/composition.rs` | Composition root |
| `data/` | Seed files: `programs.json`, `student.json` |
| `tests/fixtures/` | A districts 15 and 20 catalog for the HTTP tests |
| `migrations/` | SQLx migrations |
| `docs/` | OKF knowledge bundle; start at [`docs/index.md`](docs/index.md) |
| `scripts/`, `tests/test_docs.py` | Documentation checker and its tests |

## Checks

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m pip install -r requirements-docs.txt
python scripts/check_docs.py
python -m unittest discover -s tests -v
```

The [self-audit](docs/self-audit.md) records which checks ran, where, and
against which revision. The simulation reproduces the Python results
within Monte Carlo error; the scan output is identical.

## Sources

- NYC DOE InfoHub test results:
  <https://infohub.nyced.org/reports/academics/test-results>
- Marian, A. (2023). *Algorithmic Transparency and Accountability through
  Crowdsourcing: A Study of the NYC School Admission Lottery.* FAccT '23.
  <https://doi.org/10.1145/3593013.3594009>
- MySchools middle school catalog:
  <https://www.myschools.nyc/en/api/v2/schools/process/6/>

# nycmsa — NYC middle school admissions Monte Carlo

A Rust port of [nyc-ms-admissions-sim](https://github.com/abenamer/nyc-ms-admissions-sim)
with a web interface. It estimates one student's chances across a ranked
list of NYC middle school programs from last year's MySchools "Seats and
Demand" figures, and scans the MySchools catalog for every program the
student is eligible for.

The code follows the [REAS](https://github.com/abenamer/reas) engineering
standard: explicit domain types, ports and adapters, one composition root,
typed errors, and an OKF documentation bundle in [`docs/`](docs/index.md).

## What you can enter

- The lottery number prefix the DOE gave your child (`BD`, `b03f`, ...).
  The first two hex characters place the number to 0.4%.
- Where you expect your child to land on a talent test, as a percentile,
  with a spread for uncertainty.
- Eligibility attributes (`d15`, `frl`, `sibling_K442S`, ...) and the
  ranked list of program codes.
- Model assumptions: trials, seed, year-to-year demand noise, an attrition
  multiplier, and whether waitlists redraw the lottery number or reuse it.

## Run

```bash
cargo run --release --locked -- serve
# open http://127.0.0.1:8080
```

Command line, as in the Python original:

```bash
cargo run --release --locked -- sim --lottery B0 --twain-pct 0.25 --rank K187A K442S
cargo run --release --locked -- scan --attrs d15,d20 --lottery BD --csv scan.csv
cargo run --release --locked -- fetch data/myschools_d15_d20.json --districts 15,20
```

With Docker Compose, PostgreSQL stores the runs on a named volume:

```bash
docker compose up --build
```

See [operations](docs/operations.md) for backups, restore, migrations,
and hosting, and the [API reference](docs/api.md) for the JSON endpoints.

## Layout

| Path | Contents |
| --- | --- |
| `src/domain/` | Model and pure rules: lottery, programs, student, simulation, catalog scan |
| `src/application/` | Use cases and ports |
| `src/adapters/` | HTTP (Axum, Askama), CLI (Clap), JSON files, MySchools parser and client, in-memory and PostgreSQL run stores |
| `src/composition.rs` | Composition root |
| `data/` | `programs.json`, `student.json`, MySchools catalog snapshots |
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

- Marian, A. (2023). *Algorithmic Transparency and Accountability through
  Crowdsourcing: A Study of the NYC School Admission Lottery.* FAccT '23.
  <https://doi.org/10.1145/3593013.3594009>
- MySchools middle school catalog:
  <https://www.myschools.nyc/en/api/v2/schools/process/6/>

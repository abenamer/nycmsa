---
type: Operating Procedure
title: Operations
description: Run, deploy, back up, and recover the application.
tags: [operations, docker, postgresql]
---

# Operations

## Prerequisites

- Rust 1.99.1 with `clippy` and `rustfmt`. `rust-toolchain.toml` pins it.
- Python 3 with `requirements-docs.txt` for the documentation checks.
- Docker with Compose for the container deployment.
- PostgreSQL 16 or 17 for persistent run storage. Without it, runs stay
  in memory.

## Run locally

1. Build: `cargo build --release --locked`.
2. Start: `./target/release/nycmsa serve`.
3. Open `http://127.0.0.1:8080`.

Expected result: the log shows `listening; every endpoint is anonymous`
and `GET /healthz` returns `ok`.

To keep runs across restarts, set `DATABASE_URL` before step 2, for
example `postgres://nycmsa:secret@127.0.0.1/nycmsa`. The server applies
the migrations in `migrations/` at startup and refuses to start when the
database is unreachable.

## Run the checks

Run from the repository root:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m pip install -r requirements-docs.txt
python scripts/check_docs.py
python -m unittest discover -s tests -v
```

The PostgreSQL integration test is ignored by default. Run it against a
disposable database:

```bash
NYCMSA_TEST_DATABASE_URL=postgres://user:pass@127.0.0.1/db \
  cargo test --test postgres_runs -- --ignored
```

It applies the migrations to that database, inserts two rows, and deletes
them.

## Deploy with Docker Compose

1. Optional: set `POSTGRES_PASSWORD` in the shell or a `.env` file. The
   default password is `nycmsa` and is suitable only for a private host.
2. Run `docker compose up --build -d`.
3. Open `http://127.0.0.1:8080`.

Expected result: `docker compose ps` lists `db` as healthy and `app` as
running. The database files are in the named volume `nycmsa_pgdata`, so
`docker compose down`, `docker compose up --build`, and image rebuilds
keep the runs.

Do not run `docker compose down -v` against data you must keep. The `-v`
flag deletes the named volume. A volume is not a backup.

The Compose file publishes port 8080 on loopback only. To reach the app
from other machines, put an authenticating reverse proxy in front of it.

## Back up the database

1. Run `docker compose exec db pg_dump -U nycmsa -Fc nycmsa > nycmsa-$(date +%F).dump`.
2. Copy the file to storage independent of the Docker host.

Expected result: a file of a few kilobytes per hundred runs.

## Restore the database

1. Stop the app: `docker compose stop app`.
2. Recreate the database:
   `docker compose exec db psql -U nycmsa -d postgres -c 'DROP DATABASE nycmsa;' -c 'CREATE DATABASE nycmsa OWNER nycmsa;'`.
3. Restore: `docker compose exec -T db pg_restore -U nycmsa -d nycmsa < nycmsa-YYYY-MM-DD.dump`.
4. Start the app: `docker compose start app`.

Expected result: `GET /api/runs` lists the restored runs.

Recovery: if step 3 fails, the database is empty. Repeat step 2 and
step 3 with a different dump. The app does not start until the database
exists, because the migration step needs it.

Do a test of this procedure against a copy before you depend on it.

## Migrations

Each schema change is a new file in `migrations/` named
`<version>_<description>.sql`. The server applies pending migrations at
startup inside SQLx's migration transaction and records them in
`_sqlx_migrations`. A failed migration stops the server before it
listens; fix the SQL and restart. Never edit an applied migration file;
SQLx rejects a checksum change.

## Refresh the MySchools catalog

1. Run `nycmsa fetch data/myschools_d15_d20.json --districts 15,20`, or
   `--districts all` for the whole city.
2. Restart the server with `--catalog` pointing at the new file.

Expected result: the scan page reports the new program count. The fetch
pauses 500 ms between pages. It was not run from the development
environment because that environment has no route to `myschools.nyc`;
see the [self-audit](self-audit.md).

The simulation's `data/programs.json` is maintained by hand from the
MySchools "Seats and Demand" figures and is not refreshed by `fetch`.

## Hosting elsewhere

Any host that runs a Docker image and gives it a PostgreSQL URL works:
Cloudflare Containers, Fly.io, Railway, or a VM with Compose. Cloudflare
Workers run Rust only as WebAssembly without threads or sockets, so the
Axum server does not fit them; the pure `domain` crate would compile for
them, with a different inbound adapter.

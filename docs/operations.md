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

The server reads everything from PostgreSQL: runs, the catalog, the test
results, the hand-entered programs, and the form defaults.

1. Create a database and export its URL, for example
   `export DATABASE_URL=postgres://nycmsa:secret@127.0.0.1/nycmsa`.
2. Build: `cargo build --release --locked`.
3. Fill the database: `./target/release/nycmsa bootstrap`. It applies the
   migrations, stores `data/programs.json` and `data/student.json`,
   downloads the MySchools catalog for all districts, downloads the
   state test results, and looks up every school's MySchools page for the
   program profiles. It skips any set the database already holds.
4. Start: `./target/release/nycmsa serve`.
5. Open `http://127.0.0.1:8080`.

Expected result: the log shows `loaded reference data` with the counts,
then `listening; every endpoint is anonymous`, and `GET /healthz` returns
`ok`. The server refuses to start when the database is unreachable or
lacks a reference set; the error names the missing set.

Recovery: if `bootstrap` fails during a download, the sets stored before
the failure stay. Run it again; it continues with the missing sets.

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
   Set `NYCMSA_PORT` when host port 8080 is taken.
2. Run `docker compose up --build -d`.
3. Open `http://127.0.0.1:8080` (or the port you set).

Expected result: `docker compose ps` lists `db` as healthy, `init` as
exited with code 0, and `app` as running. On the first start `init` runs
`nycmsa bootstrap`, which downloads the catalog and the test results; it
needs a route to `myschools.nyc` and `data.cityofnewyork.us` and takes a
few minutes. The database files are in the named volume `nycmsa_pgdata`,
so `docker compose down`, `docker compose up --build`, and image rebuilds
keep every snapshot and run.

Do not run `docker compose down -v` against data you must keep. The `-v`
flag deletes the named volume. A volume is not a backup.

The local Compose file publishes port 8080 on loopback only. The
deployment below puts Caddy in front and serves the app to everyone,
which is its purpose: every page is public and anonymous.

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

1. Run `nycmsa fetch` (with Compose: `docker compose run --rm init fetch`).
2. Restart the server (`docker compose restart app`).

Expected result: the command prints `stored a catalog snapshot of N
schools`, and the server log shows the new count. The previous snapshot
stays in the database. The fetch pauses 500 ms between pages.

The server derives the simulation's programs from this catalog. The
hand-entered programs replace the derived entries of the same codes.

## Refresh the state test results

1. Run `nycmsa fetch-scores` (with Compose: `docker compose run --rm init fetch-scores`).
2. Restart the server.

Expected result: the command downloads two workbooks of about 50 MB each
from the DOE InfoHub, prints the number of schools and the years it
found, and the scan page shows ELA and Math columns for that year.
`--source open-data` uses the NYC Open Data datasets instead, which end
at 2023.

After a refresh, do the cross-check in the [self-audit](self-audit.md):
read the same workbooks with a second tool and compare the stored
values. The reader fails on a missing sheet or column, but a reordered
or renamed value column would need this check to catch it.

## Refresh the program profiles

1. Run `nycmsa fetch-profiles` (with Compose: `docker compose run --rm init fetch-profiles`).
2. Restart the server.

Expected result: the command looks up every school in the latest catalog
snapshot on MySchools (one request per school, a quarter second apart,
about three minutes for the city), prints the number of program profiles
stored, and the picker's "kind of program" filter and the "about each
program" page use the new snapshot. `bootstrap` runs this step when the
database has no profiles.

## Re-run the kind-of-program rules

When the keyword rules in `domain::catalog` change:

1. Run `nycmsa reclassify-profiles`.
2. Restart the server.

Expected result: a new profile snapshot with the same texts and new tags,
in a few seconds and without a download.

## Update the hand-entered programs or the form defaults

1. Edit `data/programs.json` or `data/student.json`.
2. Run `nycmsa import` (with Compose: `docker compose run --rm init import`).
3. Restart the server.

Expected result: the command prints the number of programs stored. The
previous set stays in the database.

## Deploy to a DigitalOcean Droplet

The `deploy/` directory holds a production Compose file that runs the
prebuilt image behind Caddy, and a cloud-init file that turns a fresh
Ubuntu Droplet into a running server without logging in. CI pushes the
image to `ghcr.io/abenamer/nycmsa` on every merge to `main` (job
`docker`, step "Push the image"), so the server never compiles anything.

Prerequisites: a DigitalOcean account with an SSH key added; the
container image public under the repository owner's packages (check at
`https://github.com/abenamer?tab=packages` after the first merge; a
private package makes the Droplet's image pull fail); and the
`production` environment in the repository settings restricted to the
`main` branch (Settings, Environments, production, Deployment branches).

1. In DigitalOcean choose Create, Droplets: region New York, image
   Ubuntu 24.04 LTS, size Basic, Regular, $6 (1 GB RAM, 25 GB disk).
   The $4 size is too small for PostgreSQL and the app together.
2. Under Authentication pick your SSH key. Under Advanced Options tick
   "Add Initialization scripts (free)" and paste the contents of
   `deploy/cloud-init.yaml`.
3. Create the Droplet. After about ten minutes open `http://<droplet IP>/`.
4. Optional, for HTTPS: point a DNS A record at the Droplet, then on the
   server set `SITE_ADDRESS=<that name>` in `/opt/nycmsa/.env` and run
   `cd /opt/nycmsa && docker compose up -d caddy`. Caddy fetches the
   certificate on the first request.

Expected result: `ssh root@<droplet IP> 'cd /opt/nycmsa && docker compose ps'`
lists `db`, `app` and `caddy` as running and `init` as exited with code 0;
`/healthz` through the proxy returns `ok`; `/pick` lists every program.
cloud-init logs to `/var/log/cloud-init-output.log`; the last line reads
"nycmsa: cloud-init finished".

What cloud-init sets up: a firewall that admits only SSH, HTTP and HTTPS;
Docker from Docker's own repository; a 1 GB swap file; unattended
security updates with a reboot at 05:30 UTC when one is needed; the files
in `/opt/nycmsa` with a random database password in `.env`; a daily
`pg_dump` at 03:10 UTC into `/opt/nycmsa/backups` (newest 14 kept); and a
daily update at 04:17 UTC (`update.sh`) that refreshes the deployment
files from `main`, pulls the newest image, and restarts only what
changed; `.env` is never touched. Run `/opt/nycmsa/update.sh` by hand to
deploy a merge at once. The image comes from the `publish` CI job, which
runs only for `main` after the other jobs pass and under the
`production` environment: once, in the repository settings, set that
environment's deployment branches to `main` only.

Recovery: if the page does not answer after fifteen minutes, read the
cloud-init log. An image pull error means the package is private (see
prerequisites); make it public and run `cd /opt/nycmsa && docker compose
up -d`. A bootstrap error means a download from MySchools, NYC Open Data
or InfoHub failed; run `docker compose run --rm init bootstrap` again.
The backups directory is on the same disk as the database, so copy the
dumps elsewhere (DigitalOcean Spaces, or `scp` to your machine). Enable
DigitalOcean's own Droplet backups for a whole-disk fallback.

To rebuild from scratch: destroy the Droplet and create a new one the
same way; restore a dump with the procedure above if you want the runs
and snapshots back. Everything else bootstrap downloads again.

## Hosting elsewhere

Any host that runs a Docker image and gives it a PostgreSQL URL works:
Fly.io, Railway, or another VM with `deploy/compose.yaml`. Cloudflare
Workers run Rust only as WebAssembly without threads or sockets, so the
Axum server does not fit them; the pure `domain` crate would compile for
them, with a different inbound adapter.

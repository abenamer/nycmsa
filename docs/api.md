---
type: API Reference
title: HTTP API and command line
description: Endpoints, their access rules and limits, and the CLI commands.
tags: [api, http, cli]
---

# HTTP API and command line

## Access

Every endpoint is anonymous. The server is a local analysis tool: it
holds no personal data beyond what the operator enters, and it binds to
`127.0.0.1:8080` unless `--bind` says otherwise. Do not expose it to the
public internet without an authenticating reverse proxy. A loopback
address is a deployment choice, not an authentication check.

## Limits

| Limit | Value | Where |
| --- | --- | --- |
| Request body | 1 MiB | `HttpLimits::body_limit_bytes` |
| Request timeout | 60 s, status 408 | `HttpLimits::request_timeout` |
| Concurrent simulations | one per CPU | `HttpLimits::cpu_permits` |
| Trials per run | 5,000,000 | `domain::simulation::MAX_TRIALS` |

## Endpoints

| Method and path | Purpose | Success | Errors |
| --- | --- | --- | --- |
| `GET /` | Questions and recent runs; `ranking=K1,K2` prefills the list from the picker | 200 HTML | |
| `POST /simulate` | Form submit | 303 to `/runs/{id}` | 422 HTML with the message |
| `GET /runs/{id}` | Report page | 200 HTML | 404 |
| `GET /scan` | Scan questions and results | 200 HTML | 422 HTML |
| `GET /pick` | Program picker: every program in a sortable table with district, kind, and screened filters; the chosen list is kept in the browser | 200 HTML | |
| `GET /profiles` | What each program is about, with tags and evidence, filtered by repeated `district` keys | 200 HTML | |
| `GET /programs` | Program definitions, filtered by repeated `district` keys | 200 HTML | |
| `POST /api/simulate` | Run a simulation | 201 JSON run | 422 |
| `GET /api/runs` | Recent runs | 200 JSON array | |
| `GET /api/runs/{id}` | One run | 200 JSON run | 404 |
| `GET /api/scan` | Scan rows | 200 JSON | 422 |
| `GET /api/scan.csv` | Scan rows as CSV | 200 `text/csv` | 422 |
| `GET /api/programs` | Program definitions | 200 JSON | |
| `GET /healthz` | Liveness | 200 `ok` | |

A repository failure gives 503. A rendering failure gives 500. Error
bodies are JSON: `{"error": "<message>"}`.

### `POST /api/simulate`

```json
{
  "student": {
    "attributes": ["d15", "d20", "mark_twain_test"],
    "ranking": ["K187A", "K239SC", "K239CM", "K442S", "K443M", "K686A"],
    "lottery_number": "BD",
    "talent_test_percentile": 0.10,
    "talent_test_sd": 0.10
  },
  "parameters": {
    "trials": 200000,
    "seed": 0,
    "demand_sd": 0.15,
    "attrition": 1.0,
    "waitlist_numbers": "redrawn"
  },
  "programs": null
}
```

- `lottery_number` is optional. Omit it or send `null` to draw a number
  in each trial.
- `talent_test_percentile` is a fraction, `0` best. Omit it to draw.
- `parameters` and each of its fields are optional; the defaults are
  shown. `waitlist_numbers` is `"redrawn"` or `"reused"`.
- `programs` optionally replaces the server's definitions for this run.
  It has the shape of the `programs` object in `data/programs.json`.

The response is the run:

```json
{
  "id": 1,
  "created_at_unix": 1793046796,
  "student": { "...": "as sent, normalized" },
  "report": {
    "parameters": { "...": "as applied" },
    "lottery_fraction": 0.73828125,
    "rows": [
      {
        "rank": 1, "code": "K187A", "name": "Academy for Scientific Research",
        "selection": "lottery", "group_name": "District 20 students and residents",
        "initial_rate": 0.132, "waitlist_rate": 0.0,
        "p_initial": 0.0, "p_waitlist": 0.002, "p_any": 0.002
      }
    ],
    "summary": {
      "p_initial_offer": 0.936, "p_any_offer": 0.981, "p_no_offer": 0.019,
      "p_lottery_offer": 0.026, "p_screened_offer": 0.970
    }
  }
}
```

### Form fields of `POST /simulate`

The HTML form asks plain questions and maps the answers to model inputs.
The JSON API exposes the inputs directly.

| Field | Values | Model input |
| --- | --- | --- |
| `lottery_number` | Hex prefix or blank | `student.lottery_number`; blank means unknown |
| `talent_test` | `not_taken`, `unknown`, `top_quarter`, `second_quarter`, `third_quarter`, `bottom_quarter` | `not_taken` removes `mark_twain_test`; the quarters set the percentile to 0.125, 0.375, 0.625, 0.875 with spread 0.10; `unknown` draws it |
| `attribute` (repeated) | District chips `d1`…`d32` and `frl` | `student.attributes` |
| `sibling_codes`, `zoned_codes`, `continuing_codes` | Comma-separated program codes | `sibling_<code>`, `zoned_<code>`, `continuing_<code>` attributes |
| `ranking` | Comma-separated codes | `student.ranking` |
| `waitlist` | `redrawn`, `reused` | `parameters.waitlist_numbers` |
| `outlook` | `as_published`, `hopeful` | `parameters.attrition` 1.0 or 1.3 |

The form fixes `trials` at 200,000, `seed` at 0, and `demand_sd` at 0.15.

### `GET /api/scan`

Query parameters: `attrs` (comma-separated) or repeated `attr` keys, `frl`
(`1`, `true`, `on`, or `yes`), `lottery` (hex prefix). With no query
string the defaults are `d15,d20`, not FRL, and `BD`. The
response lists the attributes, the FRL flag, the lottery prefix and
fraction, and the rows in reach order. Each row also carries the school's
`dbn`, `scores_year`, `ela_proficient_pct`, and `math_proficient_pct`.
The CSV has the Python scanner's columns followed by those four.

## Command line

```text
nycmsa sim   [--programs data/programs.json] [--student data/student.json]
             [--trials 200000] [--seed 0] [--demand-sd 0.15] [--attrition 1.0]
             [--waitlist-numbers redrawn|reused] [--rank CODE...] [--no-test]
             [--twain-pct 0.25] [--lottery B0] [--json]
nycmsa scan  CATALOG.json [--frl] [--attrs d15,d20] [--lottery BD] [--csv out.csv]
nycmsa fetch        [--districts all] --database-url URL
nycmsa fetch-scores [--source infohub|open-data] --database-url URL
nycmsa fetch-profiles --database-url URL
nycmsa reclassify-profiles --database-url URL
nycmsa import       [--programs data/programs.json] [--student data/student.json] --database-url URL
nycmsa bootstrap    [--programs ...] [--student ...] [--districts all]
                    [--source infohub|open-data] --database-url URL
nycmsa serve        [--bind 127.0.0.1:8080] --database-url URL
```

`--database-url` also reads `DATABASE_URL`. `RUST_LOG` sets the log
filter; the default is `info`. A failed command exits with status 1 and
prints the error chain to stderr. `sim` and `scan` read files and need no
database.

## Browser storage

The picker keeps the chosen list and its filters, and the questions page
keeps its answers, in the browser's `localStorage` under keys that start
with `nycmsa.`. Nothing is sent to the server until the form is
submitted. The Reset buttons clear those keys.

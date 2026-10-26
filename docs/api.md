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
| Runs kept in memory | `--max-runs`, default 100 | in-memory store only |

## Endpoints

| Method and path | Purpose | Success | Errors |
| --- | --- | --- | --- |
| `GET /` | Simulate form, program list, recent runs | 200 HTML | |
| `POST /simulate` | Form submit | 303 to `/runs/{id}` | 422 HTML with the message |
| `GET /runs/{id}` | Report page | 200 HTML | 404 |
| `GET /scan` | Scan form and results | 200 HTML | 422 HTML |
| `GET /programs` | Program definitions | 200 HTML | |
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

The HTML form takes percentages where the JSON API takes fractions:
`talent_test_percentile` and `talent_test_sd` are percent values, so `10`
means `0.10`.

### `GET /api/scan`

Query parameters: `attrs` (comma-separated, default `d15,d20`), `frl`
(`1`, `true`, `on`, or `yes`), `lottery` (hex prefix, default `BD`). The
response lists the attributes, the FRL flag, the lottery prefix and
fraction, and the rows in reach order. The CSV has the same columns as the
Python scanner.

## Command line

```text
nycmsa sim   [--programs data/programs.json] [--student data/student.json]
             [--trials 200000] [--seed 0] [--demand-sd 0.15] [--attrition 1.0]
             [--waitlist-numbers redrawn|reused] [--rank CODE...] [--no-test]
             [--twain-pct 0.25] [--lottery B0] [--json]
nycmsa scan  [DATA=data/myschools_d15_d20.json] [--frl] [--attrs d15,d20]
             [--lottery BD] [--csv out.csv]
nycmsa fetch [OUT=myschools_ms.json] [--districts 15,20|all]
nycmsa serve [--bind 127.0.0.1:8080] [--programs ...] [--student ...]
             [--catalog ...] [--max-runs 100] [--database-url URL]
```

`--database-url` also reads `DATABASE_URL`. `RUST_LOG` sets the log
filter; the default is `info`. A failed command exits with status 1 and
prints the error chain to stderr.

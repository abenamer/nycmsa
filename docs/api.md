---
type: API Reference
title: HTTP API and command line
description: Endpoints, their access rules and limits, and the CLI commands.
tags: [api, http, cli]
---

# HTTP API and command line

## Access

Every endpoint is anonymous, stateless, and meant to be public behind a
reverse proxy. The server binds to `127.0.0.1:8080` unless `--bind` says
otherwise. It sets no cookies and stores nothing a visitor submits: a
simulation request is answered and forgotten
([ADR 0005](decisions/0005-no-user-input-on-the-server.md)). There is no
endpoint that lists or replays earlier requests.

## Limits

| Limit | Value | Where |
| --- | --- | --- |
| Request body | 1 MiB | `HttpLimits::body_limit_bytes` |
| Request timeout | 60 s, status 408 | `HttpLimits::request_timeout` |
| Concurrent simulations | one per CPU | `HttpLimits::cpu_permits` |
| Trials per run | 5,000,000 | `domain::simulation::MAX_TRIALS` |

There is no per-client rate limit. The semaphore and the timeout bound
what one request costs, not how many requests one client sends; a busy
client can keep every CPU permit and make other visitors wait. A public
deployment that needs fairness puts the limit in the proxy in front.

## Endpoints

| Method and path | Purpose | Success | Errors |
| --- | --- | --- | --- |
| `GET /` | Questions, blank; `ranking=K1,K2` prefills the list from the picker | 200 HTML | |
| `POST /simulate` | Form submit; the result page in the same response | 200 HTML | 422 HTML with the message |
| `GET /scan` | Scan questions; results only with a query string | 200 HTML | 422 HTML |
| `GET /pick` | Program picker: every program in a sortable table with district, kind, and screened filters; the chosen list is kept in the browser | 200 HTML | |
| `GET /profiles` | What each program is about, with tags and evidence, filtered by repeated `district` keys | 200 HTML | |
| `GET /programs` | Program definitions, filtered by repeated `district` keys | 200 HTML | |
| `POST /api/simulate` | Run a simulation | 200 JSON report | 422 |
| `GET /api/scan` | Scan rows | 200 JSON | 422 |
| `GET /api/scan.csv` | Scan rows as CSV | 200 `text/csv` | 422 |
| `GET /api/programs` | Program definitions | 200 JSON | |
| `GET /healthz` | Liveness | 200 `ok` | |

A rendering failure gives 500. Error bodies are JSON:
`{"error": "<message>"}`.

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

The response is the report. It carries no identifier because nothing is
stored:

```json
{
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
The form opens blank: the server prefills no answer. The result page
carries the submitted fields in a `data-answers` attribute (the JSON
shape of the form) so the browser can keep them.

### `GET /api/scan`

Query parameters: `attrs` (comma-separated) or repeated `attr` keys, `frl`
(`1`, `true`, `on`, or `yes`), `lottery` (hex prefix). With no query
string the JSON endpoint uses the Python scanner's defaults, `d15,d20`,
not FRL, and `BD`; the HTML page shows blank questions instead. The
response lists the attributes, the FRL flag, the lottery prefix and
fraction, and the rows in reach order. Each row also carries the school's
`dbn`, `scores_year`, `ela_proficient_pct`, `math_proficient_pct`,
`latitude`, `longitude`, and `address`.
The CSV has the Python scanner's columns followed by those seven.

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

## Browser storage and privacy

Everything about a visitor lives in that visitor's browser, in
`localStorage` under keys that start with `nycmsa.`:

| Key | Holds | Written by |
| --- | --- | --- |
| `nycmsa.ranking` | The chosen program codes | picker and questions page |
| `nycmsa.pickerFilters` | The picker's filters and search text | picker |
| `nycmsa.answers` | The other answers on the questions page | questions page, on submit |
| `nycmsa.results` | The last 20 results: when, the answers, programs, lottery prefix, chance | result page |
| `nycmsa.home` | The coordinates and label of the address typed to sort by distance | picker and scan page |

Nothing is sent to the server until a form is submitted, and the server
keeps nothing from it. The one outside call the browser makes is the
address lookup for the distance column: it sends the typed address to
the city's GeoSearch service (`geosearch.planninglabs.nyc`, NYC
Department of City Planning) and keeps the coordinates it gets back.
This site's server never sees the address. Distances are straight lines
computed in the browser from each school's coordinates, which the
catalog carries. Each school's address links to Google Maps, and a
Directions link opens Google Maps transit directions with the school as
the destination and no origin: the visitor's home is never put into a
link, so it reaches Google only if the visitor types it there. No cookie is set. There is no analytics, no
visitor statistics, and no third-party script. Every page's footer says
so and has a "Forget everything this browser remembers" button that
removes every `nycmsa.` key; the Reset buttons clear subsets.

---
type: Domain Model
title: Domain model
description: The Monte Carlo model of one applicant's chances and the catalog scan rules.
tags: [domain, admissions, monte-carlo]
sources:
  - id: marian2023
    resource: https://doi.org/10.1145/3593013.3594009
    title: "Marian, A. (2023). Algorithmic Transparency and Accountability through Crowdsourcing: A Study of the NYC School Admission Lottery. FAccT '23."
  - id: myschools
    resource: https://www.myschools.nyc/en/api/v2/schools/process/6/
    title: MySchools middle school catalog API
---

# Domain model

The domain lives in `src/domain/`. It has no I/O and no framework types.
Random values enter only through a seed.

## Background

NYC assigns middle school seats with a deferred-acceptance match. Each
student receives one random lottery number, a 32-character hexadecimal
string compared left to right from `0` to `f`. The DOE uses it to order
students inside a priority group when the group has more applicants than
seats [marian2023]. The first two characters place a number in the
population to a precision of 1/256, about 0.4%, under the uniform
distribution the generator produces.

From one applicant's point of view, each program has a cutoff inside the
student's priority group. The student is matched to the highest-ranked
program whose cutoff their number beats. Waitlists exist only for programs
ranked above the match.

## Values and invariants

| Type | Invariant | Constructor |
| --- | --- | --- |
| `LotteryPrefix` | 1 to 32 hexadecimal characters, lowercase, no hyphens | `LotteryPrefix::parse` |
| `UnitInterval` | Finite, in `[0, 1]` | `UnitInterval::new` |
| `StandardDeviation` | Finite, not negative | `StandardDeviation::new` |
| `Attrition` | Finite, greater than 0 | `Attrition::new` |
| `Trials` | 1 to 5,000,000 | `Trials::new` |
| `ProgramCode` | ASCII letters and digits, uppercase | `ProgramCode::parse` |
| `Attribute` | Non-empty, no whitespace | `Attribute::parse` |
| `GroupDemand` | `initial_offers <= offers <= applicants` | `GroupDemand::new` |
| `Ranking` | Non-empty, each code once | `Ranking::new` |
| `DistrictCode` | Non-empty | `DistrictCode::parse` |

`LotteryPrefix::fraction` gives the share of the lottery space before the
prefix. `B0` gives `0xB0 / 0x100 = 0.6875`.

## Programs

A `Program` has a code, a name, a `SelectionKind`, and ordered
`PriorityGroup`s. A student falls into the first group whose required
attributes they all hold. Each group carries last year's `GroupDemand`.
Published offers include waitlist offers, so `offers - initial_offers`
estimates the waitlist offers.

`SelectionKind::Lottery` ranks the group by the shared lottery number.
`SelectionKind::Screened` ranks by a talent test and is modelled as an
independent percentile draw.

## The simulation

`simulate` runs `Parameters::trials` trials with a ChaCha generator seeded
from `Parameters::seed`. Per trial:

1. Draw the lottery number `L` as `U(0, 1)`, or use the known prefix
   fraction. Lower is better. One number serves every lottery program.
2. Draw the talent-test base percentile `T` as `U(0, 1)`, or use the known
   value.
3. For each ranked program with an eligible group, draw the cutoff as
   `Beta(offers + 1, applicants - offers + 1)`, multiply by
   `exp(N(0, demand_sd))` and by `attrition`, and clip to `[0, 1]`.
   A lottery program clears when `L` is below the initial-round cutoff.
   A screened program clears when `T + N(0, talent_test_sd)`, clipped,
   is below it.
4. The initial match is the first cleared program.
5. Waitlist. A lottery program ranked above the match gives a waitlist
   offer when the waitlist number is below the waitlist cutoff drawn from
   `Beta(waitlist_offers + 1, remaining - waitlist_offers + 1)`.
   With `WaitlistNumbers::Redrawn`, the waitlist number is a fresh
   `U(0, 1)`. With `WaitlistNumbers::Reused`, it is the student's position
   among the applicants who missed the initial round,
   `(L - cutoff) / (1 - cutoff)`. A screened program gives a waitlist offer
   when `T` is below the cutoff drawn from the full offer count.

The report gives, for each program, the share of its group matched in the
initial round, the share of the remaining group reached by the waitlist,
and the probabilities of an initial offer, a waitlist offer, and either.
The summary gives the probability of an initial-round offer, of any offer,
of no offer, and of an offer from a lottery or a screened program.

## Waitlist assumption

The DOE's published policy is a new random number for each waitlist. In
July 2022 the DOE confirmed that a subset of schools had not redrawn the
numbers [marian2023]. The default, `Redrawn`, follows the published policy
and the Python original. `Reused` is the user's alternative assumption.
See [ADR 0003](decisions/0003-waitlist-number-assumption.md).

## Differences from the Python original

- Same model and same distributions. The random streams differ (NumPy
  PCG64 against ChaCha8), so the two programs give the same results within
  Monte Carlo error, not the same numbers.
- The Python original clamped `initial_offers` to `offers`. The Rust
  version rejects `initial_offers > offers` and `offers > applicants` as
  invalid input.
- A group with zero applicants gives an initial rate of zero instead of a
  division error.
- The talent-test fields are `talent_test_percentile` and `talent_test_sd`.
  The JSON files still accept `mark_twain_percentile` and `mark_twain_sd`.
- `WaitlistNumbers` is new.

## Not modelled

- The applicant's rank among a finite applicant pool. The report treats
  the lottery fraction as the exact percentile. For a program with a few
  hundred applicants, the realised percentile moves by one or two points
  [marian2023]. The demand noise term is larger than this effect.
- Academic groups at screened programs that rank by course grades. The
  program notes in `data/programs.json` record this limit.
- Students with disabilities (SWD) seats and figures.
- Changes in the applicant pool or the admissions rules between years,
  beyond the demand noise and attrition multipliers.

## Programs derived from the catalog

The web server simulates every program in the MySchools catalog that
published seats or applicants, not only the hand-entered ones.
`programs_from_catalog` builds them:

- Open and zone-priority programs rank by lottery number. Screened,
  talent test, audition, and language programs are modelled as a screened
  test: the student's expected placement decides, not the lottery number.
- Each MySchools priority group becomes one simulation group, or two when
  the DOE reports its figures for free or reduced price lunch students
  separately: the FRL slice first (it also requires `frl`), then the rest.
- Group rules become required attributes: `d15` for a district, a borough
  attribute such as `borough_brooklyn` for a borough (derived from the
  district attributes), and per-program attributes `sibling_K442S`,
  `zoned_K062Z`, `continuing_K686A`, `attends_K686A` for sibling, zone,
  continuing-student, and current-student rules. A group whose rule the
  parser does not recognise admits nobody.
- Offers are capped at applicants; both are rounded in the source text.
- First-round offers are an estimate: the seats are filled in priority
  order, each group taking `min(offers, seats still open)`. The rest of a
  group's offers count as waitlist offers. The hand-entered
  `data/programs.json` keeps the DOE's own split where it was read from
  the screenshots, and its programs replace the derived ones of the same
  code.

District codes drop leading zeros, so the school code `09` and the group
text "District 9" both give `d9`.

## State test results

The scan page shows, for each school, the share of its students at
proficiency (levels 3 and 4) on the New York State ELA and Math tests, for
the latest year NYC Open Data holds (2023 at the time of writing). The
figures are keyed by the school's DBN and are informational; the
simulation does not use them.

## Catalog scan

`scan` reads a list of `CatalogProgram`s from the MySchools catalog. For
each program with published seats or applicants, it selects the first
group whose `PriorityCriterion` admits the student, reads the FRL or
non-FRL slice of that group's figures, and classifies the program:

| `Reach` | Condition |
| --- | --- |
| `AllOffered` | Every applicant in the slice received an offer |
| `Likely` | Offer share is at least the lottery fraction |
| `WaitlistMaybe` | Offer share is at least 40% |
| `No` | Offer share is below 40% |
| `ByScreen` | The program does not rank by lottery number |
| `SeatsOpen` | No figures and seats stayed open |
| `Unknown` | No figures and all seats were filled |

Rows sort by offer share, with open-seat programs first among the rows
without a share. The parsing of MySchools prose into figures and of group
names into criteria is adapter work in `src/adapters/myschools.rs`. Unlike
the Python scanner, borough groups ("Bronx students and residents") admit
a student from any district in that borough, so five District 15 and 20
programs classify differently.

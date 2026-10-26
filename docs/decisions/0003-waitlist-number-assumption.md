---
type: Architecture Decision
title: Waitlist number assumption
description: Why the waitlist lottery number is a user input.
tags: [decision, domain]
sources:
  - id: marian2023
    resource: https://doi.org/10.1145/3593013.3594009
    title: "Marian, A. (2023). Algorithmic Transparency and Accountability through Crowdsourcing. FAccT '23, section 4.5."
---

# ADR 0003 — Waitlist number assumption

## Context

The Python original draws a fresh waitlist number for every lottery
program, following the DOE's published policy: "For waitlists, each
applicant receives a new random number for each waitlist they are on."
In July 2022 the DOE confirmed that for a subset of schools the numbers
had not been redrawn [marian2023]. Which behavior applies in a given year
is not known in advance.

## Decision

Add `WaitlistNumbers` to the simulation parameters with two values.
`Redrawn` keeps the Python behavior and is the default. `Reused` keeps
the student's number: their waitlist position among the applicants who
missed the initial round is `(L - cutoff) / (1 - cutoff)`, compared with
the waitlist offer share of that remaining group.

Expose the choice in the web form, the JSON API, and the CLI.

## Consequences

- Users can bracket their waitlist chances under both assumptions.
- A reused bad number gives near-zero waitlist chances at popular
  programs; the unit test `a_reused_bad_number_stays_bad_on_the_waitlist`
  pins this.
- Screened programs are unchanged: they rank waitlists by test score.

---
type: Architecture Decision
title: Ports and adapters for a public project
description: Answers to REAS-ARCH-003 and the selected boundaries.
tags: [decision, architecture]
---

# ADR 0001 — Ports and adapters for a public project

## Context

The Python original was three scripts and two JSON files. The Rust port
adds a web interface, persistent storage, and a container deployment. The
repository is public.

REAS-ARCH-003 asks four questions before the architecture is selected:

1. Who will read and maintain the code? The owner, with a Symfony and
   Python background, and AI coding agents. Readers who know ports and
   adapters from Symfony services and interfaces should find the layout
   familiar.
2. What external systems does it use? The MySchools API, PostgreSQL, the
   file system, and the system clock.
3. For how long must the project remain in use? Across admissions
   cycles, with yearly data refreshes, for as long as the owner has
   children in the NYC middle school process and the public finds the
   tool useful. The service life is years, not one execution.
4. How likely are the boundaries to change? The run store already has two
   implementations. The catalog source may gain a live-API adapter. The
   simulation model will change as the DOE changes its rules.

## Decision

Use ports and adapters with one composition root per executable.
Declare ports where a second implementation or a test double exists or is
expected: `CatalogStore`, `Clock`, `CatalogDownloader`. Do not declare a
port for the catalog file loader or the program definitions loader: each
has one source, and the use cases take the loaded values directly.

Keep the domain free of frameworks. Put serialization in `adapters::dto`.

## Consequences

- Each operation traces inbound adapter → use case → port → outbound
  adapter. The HTTP tests exercise the real router with the in-memory
  store.
- Adding a live catalog adapter means a new port and a small change in
  the composition root.
- The generic use-case types make the HTTP state type long. The
  composition root keeps the concrete types visible; handlers are generic.

## Alternatives

A flat module layout, as REAS permits for a private personal tool, was
rejected because the repository is public and already has two run stores.

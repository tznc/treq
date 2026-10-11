# Product Requirements

This directory contains Treq product requirement documents.

## Status values

- **Draft** — proposed behavior is still being designed.
- **Active** — this is the current product contract and implementation may still be in progress.
- **Delivered** — the product contract is implemented and retained because it remains useful as a durable specification.
- **Superseded** — retained only when another document explicitly replaces it and historical context is still valuable. Prefer Git history over keeping obsolete PRDs indefinitely.

## What belongs in a PRD

PRDs describe:

- user-visible product behavior;
- goals and non-goals;
- durable domain and architecture constraints that materially affect the product;
- ship-blocking acceptance criteria;
- explicit future considerations when current design must remain extensible for them.

PRDs do **not** serve as implementation journals.

The following belong in code, tests, PRs, or Git history instead:

- phase-by-phase completion logs;
- lists of merged PRs or commits;
- temporary test-environment status;
- bug-fix narratives;
- reverted implementation approaches;
- implementation-specific test matrices;
- operational runbooks.

A PRD should change when the intended product contract changes, not merely because implementation progressed.

## Current PRDs

- [Remote Development](./remote-development.md) — remote repositories, managed compute, user-managed SSH, review/mutations, agents, terminals, trust, and reconnect behavior.
- [Mobile](./mobile.md) — Android/iOS product behavior built on the shared Tauri application and Remote Development.
- [Multi-Repository](./multi-repo.md) — supporting repositories in a main repository's window: sidebar groups, active repository, cross-repository agents, and the `treq -r` flag.
- [Marketing](./marketing.md) — go-to-market plan: audiences, product-truth rules for copy, channels, content engine, phase gates, and measurement.
- [Billing and Teams](./billing-and-teams.md) — Pro and Team plans, entitlement, Stripe checkout and webhook, server-side enforcement, and organizations.

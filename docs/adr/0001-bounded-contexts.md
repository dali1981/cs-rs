# ADR-0001: Use bounded contexts, not one giant domain

Status: Accepted
Date: 2026-04-07

## Context

`cs-domain` currently acts as a catch-all crate containing market data infrastructure,
campaign scheduling, accounting, attribution, trade execution ports, expiration logic,
rolling logic, rules evaluation, and domain entities — all in one flat namespace.

Evidence of the problem today:

- `cs-domain/src/lib.rs` re-exports ~60 public symbols spanning at least 5 distinct concerns.
- `cs-domain/src/infrastructure/` contains repository adapters (`finq_options_repo.rs`,
  `ib_options_repo.rs`, `demo_repos.rs`) that have direct file I/O and Polars dependencies,
  sitting inside the same crate as `EarningsEvent` and `TradingCampaign`.
- `cs-backtest` imports `cs-domain::*` wholesale, which makes every refactor a grep exercise.
- When a new concept is added (e.g., `IronButterflyConfig`, `MultiLegStrategyConfig`), it lands
  in `value_objects.rs` regardless of which context owns it.

This causes refactor drift: contributors add to the nearest existing file rather than to the
correct bounded context, because the boundaries are not enforced.

## Decision

We model the system as a **modular monolith with explicit bounded contexts**:

| Bounded context      | Owns                                                          |
|---------------------|---------------------------------------------------------------|
| **Market Data**      | Options chains, equity bars, IV surfaces, spot prices        |
| **Strategy**         | Strike selection, spread construction, option legs           |
| **Campaign**         | Trading periods, scheduling, event sequencing                |
| **Execution**        | Session execution, trade lifecycle, roll logic               |
| **Accounting**       | PnL, margin, BPR, attribution, trading costs                 |
| **Application**      | Use case orchestration, repository wiring, config assembly   |

Each bounded context has its own module boundary. Translation types are allowed at context
boundaries — they are not a sign of failure, they are the correct design.

## Consequences

- `cs-domain` will stop being a catch-all. New types must declare which bounded context they
  belong to before being added.
- Infrastructure adapters (`FinqOptionsRepository`, `IbOptionsRepository`, `DemoOptionsRepository`)
  do not belong in `cs-domain`. They belong in the Application context or a dedicated
  infrastructure crate.
- `pub use *` re-exports that span multiple contexts will be removed incrementally.
- When adding a new concept, the first question is: **which bounded context owns this?**
- Types that appear in two contexts require an explicit mapping, not a shared import.

## Alternatives considered

- **Keep one large domain crate**: Already tried; results in the current state.
- **Split into many Cargo crates now**: Too disruptive before the domain is well understood.
  Modular monolith with enforced module boundaries is the right stepping stone.
- **Only fix the tests, ignore the semantic issue**: Solves symptoms, not cause.

## Non-goals

- This ADR does not require a mass rename or crate restructure immediately.
- This ADR does not prohibit `cs-domain` from existing — it constrains what it is allowed to mean.
- This ADR does not define a plugin system or dynamic dispatch hierarchy.

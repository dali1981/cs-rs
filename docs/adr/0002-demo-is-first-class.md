# ADR-0002: Demo path is a first-class product boundary

Status: Accepted
Date: 2026-04-07

## Context

The `demo` feature allows `cs` to run without any external data subscription by loading
NVDA fixture data from `fixtures/`. This is the primary path a new contributor or evaluator
uses to see the system work.

Current state of the demo path:

- `DemoOptionsRepository`, `DemoEquityRepository`, `DemoEarningsRepository` exist in
  `cs-domain/src/infrastructure/demo_repos.rs` and are always compiled.
- Fixture files live in `fixtures/` at the repo root.
- The demo command is documented in the README with specific date ranges.
- There are **no automated tests** that verify the demo command succeeds end-to-end.
- The README commands are not verified against actual behavior. They can silently go stale
  when strategy defaults, flag names, or output formats change.
- The demo repositories use `DEMO_FIXTURES_DIR` env var to locate fixtures, but this is not
  documented in a machine-verifiable way.

The consequence: the demo path breaks silently and is only discovered manually. This is a
product-quality regression, not a developer inconvenience.

## Decision

The demo path is a **supported product boundary**. It must be treated with the same rigor as
a public API.

Rules that follow from this decision:

1. **Smoke tests are required.** At minimum one integration test that runs:
   ```
   cs backtest --conf configs/demo.toml --start 2024-11-06 --end 2024-11-20
   ```
   and asserts successful exit.

2. **Fixtures are locked.** The files in `fixtures/` are not regenerated without a deliberate
   decision. Fixture changes require a corresponding smoke test update.

3. **Demo config is versioned.** `configs/demo.toml` is a committed file. Changes to it must
   be intentional and reviewed, not accidental side effects of a refactor.

4. **README commands must match.** Any command shown in the README for the demo path must be
   the exact command verified by the smoke test. If the command changes, the README changes
   with it in the same PR.

5. **Demo repositories are tested independently.** `DemoOptionsRepository::new()` must be
   loadable in a unit test that asserts non-empty data without running the full backtest.

## Consequences

- Adding a smoke test for the demo path is now a prerequisite for any PR that touches the
  backtest execution path, CLI argument parsing, or config loading.
- When `EarningsEvent`, `TradingCampaign`, or strategy default values change, the smoke test
  will catch breakage before merge.
- The fixtures directory becomes an artifact with an explicit owner, not a directory that
  happens to exist.
- Demo breakage is treated as high severity, equivalent to a broken public API.

## Alternatives considered

- **Trust manual testing**: Already the current state. The README commands are not validated.
  This is why they break.
- **Gate demo behind a CI job only**: A CI-only smoke test still requires writing the test.
  The decision here is that the test must exist; where it runs is secondary.
- **Remove the demo path**: This would eliminate the only zero-dependency way to evaluate
  the system. Not acceptable.

## Non-goals

- This ADR does not require fixture regeneration on every data update.
- This ADR does not require the demo path to cover all strategies — one representative path
  (NVDA calendar spread around earnings) is sufficient.
- This ADR does not specify the test framework (integration test, shell script, or
  `assert_cmd` are all acceptable).

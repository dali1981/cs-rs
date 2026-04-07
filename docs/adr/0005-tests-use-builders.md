# ADR-0005: Tests use builders and fixtures, not direct struct initialization

Status: Accepted
Date: 2026-04-07

## Context

Across the test suite, domain aggregates are constructed by directly initializing struct
fields. Examples observed in the codebase:

```rust
// cs-backtest/tests/test_crbg_execution.rs
let earnings_event = EarningsEvent {
    symbol: "NVDA".to_string(),
    ...
};

// cs-domain/src/rules/event.rs (test helper)
fn mock_event(symbol: &str, market_cap: Option<u64>) -> EarningsEvent {
    EarningsEvent {
        symbol: symbol.to_string(),
        ...
    }
}

// cs-backtest/src/rules/evaluator.rs (duplicate mock helper)
fn mock_event(symbol: &str, market_cap: Option<u64>) -> EarningsEvent {
    EarningsEvent {
        symbol: symbol.to_string(),
        ...
    }
}
```

This creates two problems:

**Problem 1 — Fragility.** When `EarningsEvent` gains a field (e.g., `market_cap` was added
at some point), every direct struct initializer in tests becomes a compile error. There are
currently at least 10 direct `EarningsEvent { ... }` initializers across test files and test
helpers. Adding one field requires touching all of them.

**Problem 2 — Duplication without a seam.** The same mock construction logic exists in
`cs-domain/src/rules/event.rs` and `cs-backtest/src/rules/evaluator.rs`. These are not
coordinated. When one diverges from the other, tests in different crates use different
default values for the same concept, making cross-crate test behavior unpredictable.

`TradingCampaign` construction in tests has the same pattern:
```rust
// cs-domain/src/campaign/campaign.rs (test section)
let campaign = TradingCampaign {
    symbol: "AAPL".to_string(),
    ...
};
```

These direct initializers are a sign that no builder or factory exists to serve as a stable
construction seam.

## Decision

**Tests construct domain aggregates through builders, not struct literals.**

Rules:

1. **Builders are the canonical construction path.** For each domain aggregate used in tests
   (`EarningsEvent`, `TradingCampaign`, `CalendarSpread`), a builder exists that:
   - provides sensible defaults for all required fields
   - exposes named setter methods for the fields tests actually care about
   - is the only place in the test surface that knows the struct's field names

2. **Builders live near their aggregate.** `EarningsEventBuilder` lives alongside or within
   `cs-domain`. It is not a test-only type hidden in one crate's test module.

3. **Mock helpers are replaced by builders.** The `mock_event()` function in `cs-domain` and
   the duplicate in `cs-backtest` are replaced by a single `EarningsEventBuilder::default()`.

4. **Fixture files are owned by the demo path.** Shared fixture data (e.g., NVDA options
   parquet) is maintained in `fixtures/` and loaded through the demo repositories. Tests that
   need realistic data use the demo repositories, not hand-crafted structs.

5. **Direct struct init remains acceptable in builders and in the domain itself.** The builder
   is the only code allowed to do `EarningsEvent { ... }`. Everywhere else uses the builder.

## Consequences

- Changing `EarningsEvent` shape now requires updating one builder, not 10+ direct
  initializers in test files.
- Adding a new required field to a domain aggregate requires one change in the builder and
  zero changes in test files (the builder's default handles it).
- Test intent becomes clearer: `EarningsEventBuilder::default().symbol("NVDA").build()` says
  exactly which fields the test cares about and relies on defaults for everything else.
- The duplicate `mock_event()` functions are eliminated; `cs-backtest` tests depend on the
  same construction path as `cs-domain` tests.
- When a new developer writes a test, the answer to "how do I create an EarningsEvent?" is
  unambiguous: use the builder.

## Alternatives considered

- **Add `#[derive(Default)]` to aggregates**: Works only if all fields have obvious defaults,
  and it exposes internal defaults as a public API. Builders are more explicit about what
  the default means for testing purposes.
- **Use `..Default::default()` struct update syntax**: Still requires `Default` on the
  aggregate, and still couples tests to the struct's field layout when the non-default fields
  are specified.
- **Keep direct struct init everywhere**: The current state. Already causes the fragility
  described in Context. Rejected.
- **Generate builders with a macro**: Acceptable if the builder pattern proves repetitive,
  but starts with hand-written builders to validate the interface before introducing macros.

## Non-goals

- This ADR does not require builders for production code paths — only for test construction.
- This ADR does not prohibit `..Default::default()` in tests where the aggregate already has
  a stable `Default` impl that represents a meaningful test default.
- This ADR does not mandate a specific builder crate (e.g., `derive_builder`). Hand-written
  builders are the default until proven insufficient.

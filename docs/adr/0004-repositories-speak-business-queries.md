# ADR-0004: Repository traits express business queries, not storage mechanics

Status: Accepted
Date: 2026-04-07

## Context

The repository traits in `cs-domain/src/repositories.rs` define the domain's data access
contract. They are currently a mix of business queries and storage-aware operations:

Business-query-shaped methods (good):
- `EarningsRepository::load_earnings(start, end, symbols)` — domain intent is clear
- `EquityDataRepository::get_spot_price(symbol, time)` — domain intent is clear
- `OptionsDataRepository::get_available_expirations(underlying, as_of_date)` — domain intent clear

Storage-aware methods that leak implementation detail:
- `OptionsDataRepository::get_option_bars(underlying, date)` — "bars" is a storage concept
- `OptionsDataRepository::get_option_minute_bars(underlying, date)` — "minute bars" is a
  storage granularity concept; the domain should ask for "options chain at a point in time",
  not for "minute bars"
- `OptionsDataRepository::get_option_bars_at_or_after_time` exists because some adapters
  can't find data at the exact time and need to look forward — this adaptation logic has
  leaked into the trait itself via a `max_forward_minutes` parameter

The consequence is that adapters (`FinqOptionsRepository`, `IbOptionsRepository`,
`DemoOptionsRepository`) implement methods that are named after Finq's storage format ("bars"),
not after what the domain actually needs. When a new provider is added, the author has to
understand Finq's data model to implement the repository interface.

A secondary issue: `RepositoryError` is defined in `cs-domain/src/repositories.rs` alongside
the traits. This is correct — it is a domain error. But it currently includes `Polars(String)`
as a variant, which leaks the storage technology into the error type that domain code handles.

## Decision

**Repository traits are named and parameterized around domain intent.**

Rules:

1. **Method names use domain language.** "Load earnings for a date range" is domain language.
   "Get option minute bars" is storage language. The trait uses domain language; the adapter
   translates.

2. **Adaptation logic stays in adapters.** If a provider cannot find data at an exact
   timestamp and needs to search forward, that is the adapter's concern. The trait method
   signature does not grow `max_forward_minutes` parameters. The adapter returns a
   `RepositoryError::NotFound` and the caller decides how to handle it.

3. **`RepositoryError` does not name storage technologies.** `Polars(String)` is replaced
   with `StorageError(String)` or the error message absorbs the detail. The domain handles
   `RepositoryError::NotFound`, `RepositoryError::Parse`, `RepositoryError::Storage` — not
   `RepositoryError::Polars`.

4. **New provider adapters implement the trait without learning the old provider's format.**
   If an adapter for a new data vendor requires understanding how Finq names its parquet
   columns, the trait boundary has failed.

## Consequences

- Adding a new data provider (e.g., a live broker feed) requires only implementing the
  repository trait, not understanding Finq's storage schema.
- Domain use cases (`BacktestUseCase`, `EarningsAnalysisUseCase`) become testable with a
  simple in-memory stub that implements the trait — no Polars, no parquet required.
- `DemoOptionsRepository` becomes the reference adapter: it implements the trait using
  fixture parquet files without exposing that detail to the domain.
- `get_option_bars` and `get_option_minute_bars` will be renamed in a follow-up refactor to
  reflect what the domain asks for, not how the data is stored.
- `RepositoryError::Polars` will be replaced with `RepositoryError::Storage` in a follow-up.

## Alternatives considered

- **Keep storage-aware method names**: Easier in the short term. Results in every new adapter
  being written to match Finq conventions even if the new provider has nothing to do with Finq.
- **Remove repository traits and use concrete types**: Removes testability and makes the demo
  mode impossible to implement cleanly.
- **Use a generic `query(Q) -> R` trait**: Over-abstracted for this codebase. Concrete method
  names with domain meaning are better.

## Non-goals

- This ADR does not require renaming all methods immediately.
- This ADR does not define which Polars operations are acceptable inside adapters — adapters
  may use whatever storage technology they need, as long as the trait surface stays clean.
- This ADR does not prohibit async methods on repository traits.

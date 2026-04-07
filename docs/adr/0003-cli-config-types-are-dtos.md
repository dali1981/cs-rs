# ADR-0003: CLI and config types are DTOs, not domain entities

Status: Accepted
Date: 2026-04-07

## Context

The system has three layers of type definitions for the same concept:

1. **CLI args** — `BacktestArgs`, `CampaignArgs`, `StrategyArgs` in `cs-cli/src/args/`
   (clap-derived, `String` and primitive fields)
2. **Config structs** — `CampaignConfig` in `cs-backtest/src/campaign_config.rs`
   (mix of `PathBuf`, `NaiveDate`, domain types like `OptionStrategy`, `TimingConfig`)
3. **Domain entities** — `TradingCampaign`, `EarningsEvent`, `TradingPeriod` in `cs-domain`

The current problem is that layer 2 (`CampaignConfig`) is pulling in domain types
(`OptionStrategy`, `PeriodPolicy`, `ExpirationPolicy`) directly as struct fields, and is also
pulling in infrastructure concerns (`PathBuf data_dir`, `EarningsSourceConfig`).

This means:
- `CampaignConfig` is neither a clean DTO nor a clean domain object. It is a config blob that
  knows both where data lives on disk and what trading strategy to run.
- The translation from CLI args into domain commands happens ad-hoc inside
  `BacktestConfigBuilder`, which mixes path resolution, default injection, TOML merging, and
  domain type conversion in one place.
- `clap` lives only in `cs-cli` (correctly), but TOML deserialization types in `cs-backtest`
  carry the same semantic problem: they are parsing types with domain types embedded.

## Decision

**CLI/config types are Data Transfer Objects (DTOs).** They are not domain entities and must
not be treated as such.

Concrete rules:

1. **DTOs carry raw values only.** `String`, `PathBuf`, `NaiveDate`, `bool`, `Option<T>`,
   `Vec<T>` where T is also a DTO primitive. Domain enums (`OptionStrategy`, `PeriodPolicy`)
   are allowed as DTO fields only when they have a `serde` representation — they remain
   domain types, the DTO just borrows them.

2. **Mapping is explicit.** The translation from DTO to an application command (a plain Rust
   struct with no parsing dependencies) is a named function or `impl From<Dto> for Command`.
   It is not spread across multiple builders.

3. **clap traits stay in `cs-cli`**. The wrapper pattern already exists for `SpreadTypeArg`
   and `SelectionTypeArg`. This pattern must be applied consistently. No `ValueEnum` or
   `Args` derives outside `cs-cli`.

4. **Config files (TOML) produce DTOs.** A TOML file is parsed into a struct that is then
   mapped into an application command. The TOML struct is not the command.

5. **Application commands are the stable handoff point.** What `cs-cli` hands to `cs-backtest`
   use cases must be a command struct, not a raw config blob. The command struct is the
   boundary — it is what the use case tests are written against.

## Consequences

- `CampaignConfig` will be split: the parsing/resolution concerns become a DTO, the domain
  configuration becomes an application command passed to the use case.
- Changing `EarningsSourceConfig` (an I/O concern) will no longer require touching domain
  types.
- Adding a new CLI flag requires: (a) add to args struct, (b) add to DTO if TOML-parseable,
  (c) map in the builder/converter, (d) consume in the command. The steps are explicit.
- Tests for use cases are written against command structs. They do not import `clap` or
  TOML parsing.

## Alternatives considered

- **Use `CampaignConfig` as the universal config type**: Currently how it works. Results in
  the mixed-concern type described in Context.
- **Put domain types directly on CLI args**: Tempting for small commands but couples parsing
  to the domain model. Breaks when domain types need to change.
- **Generate everything from one config schema**: Overengineered for this codebase size.

## Non-goals

- This ADR does not require a new crate for DTOs.
- This ADR does not prohibit domain enums from appearing in TOML — serde derives on domain
  enums are acceptable. The constraint is on parsing infrastructure traits (`ValueEnum`,
  `Args`, `Parser`).
- This ADR does not require rewriting all commands immediately.

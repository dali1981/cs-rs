#!/usr/bin/env bash
# Canonical demo command — single source of truth.
#
# This file is the authoritative definition of the demo backtest invocation.
# It is used by:
#   - README.md  (quoted verbatim)
#   - cs-cli/tests/demo_smoke.rs (reads DEMO_* variables below)
#   - .github/workflows/architecture-fitness.yml (demo-smoke job)
#
# DO NOT change dates or args here without updating README.md and the smoke test.

# ── Canonical demo parameters ────────────────────────────────────────────────
export DEMO_CONF="configs/demo.toml"
export DEMO_START="2024-08-14"
export DEMO_END="2024-08-28"
# ─────────────────────────────────────────────────────────────────────────────

# When executed directly, run the demo.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail
  cd "$(dirname "${BASH_SOURCE[0]}")/.."
  cargo run --release --no-default-features --features demo -p cs-cli --bin cs -- \
    backtest \
    --conf  "$DEMO_CONF" \
    --start "$DEMO_START" \
    --end   "$DEMO_END"
fi

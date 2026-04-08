#!/usr/bin/env bash
# Architecture fitness checks — run locally before pushing.
#
# Mirrors the arch-guards job in .github/workflows/architecture-fitness.yml.
# Usage: bash scripts/check_arch.sh
#
# Exit code: 0 = all guards passed, 1 = at least one guard failed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

fail=0
pass=0

guard() {
  local label=$1
  shift
  if eval "$@"; then
    echo "  OK   $label"
    pass=$((pass + 1))
  else
    echo "  FAIL $label"
    fail=$((fail + 1))
  fi
}

echo "=== Architecture fitness checks ==="
echo

# ─────────────────────────────────────────────────────────────────
# 1. No polars in cs-domain outside infrastructure/
# ─────────────────────────────────────────────────────────────────
echo "--- Dependency direction ---"
guard "no polars in cs-domain outside infrastructure/" \
  '! grep -rn "use polars" cs-domain/src/ --include="*.rs" --exclude-dir=infrastructure -q'

# ─────────────────────────────────────────────────────────────────
# 2. No clap in non-CLI crates
# ─────────────────────────────────────────────────────────────────
guard "no clap:: in cs-domain or cs-backtest" \
  '! grep -rn "clap::\|use clap" cs-domain/src/ cs-backtest/src/ --include="*.rs" -q'

echo

# ─────────────────────────────────────────────────────────────────
# 3. Deprecated vocabulary has not reappeared
# ─────────────────────────────────────────────────────────────────
echo "--- Vocabulary protection ---"
guard "Straddle alias not reintroduced"           '! grep -rn "pub type Straddle"           cs-domain/src/  --include="*.rs" -q'
guard "ShortIronButterfly alias not reintroduced" '! grep -rn "pub type ShortIronButterfly" cs-domain/src/  --include="*.rs" -q'
guard "TradingStrategy trait not reintroduced"    '! grep -rn "pub trait TradingStrategy"   cs-domain/src/  --include="*.rs" -q'
guard "StraddleStrategy alias not reintroduced"   '! grep -rn "pub type StraddleStrategy"   cs-backtest/src/ --include="*.rs" -q'

echo

# ─────────────────────────────────────────────────────────────────
# 4. Warning budget snapshot (informational — not a failure gate)
# ─────────────────────────────────────────────────────────────────
echo "--- Warning budget (informational) ---"
warn_domain=$(cargo check -p cs-domain 2>&1 | grep -c "^warning:" || true)
warn_backtest=$(cargo check -p cs-backtest 2>&1 | grep -c "^warning:" || true)
warn_cli=$(cargo check -p cs-cli 2>&1 | grep -c "^warning:" || true)
echo "  cs-domain:   $warn_domain warning(s)"
echo "  cs-backtest: $warn_backtest warning(s)"
echo "  cs-cli:      $warn_cli warning(s)"

echo
echo "=== Results: $pass passed, $fail failed ==="

[ "$fail" -eq 0 ]

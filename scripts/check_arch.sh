#!/usr/bin/env bash
# Architecture fitness checks — run locally before pushing.
#
# Runs all architectural invariant checks:
#   1. Dependency direction (cargo metadata — structural, not grep)
#   2. Public API purity (no infra types in domain public signatures)
#   3. Vocabulary protection (deprecated aliases have not reappeared)
#   4. Warning budget (current counts must not exceed baseline)
#
# Usage:  bash scripts/check_arch.sh
# Exit:   0 = all checks passed, 1 = at least one failed.

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

overall_fail=0

section() { echo; echo "─── $1 ───"; }
pass() { echo "  OK   $1"; }
fail() { echo "  FAIL $1"; overall_fail=1; }

# ─────────────────────────────────────────────────────────────────────────────
# 1. Dependency direction (structural — cargo metadata)
# ─────────────────────────────────────────────────────────────────────────────
section "1. Dependency direction"
if python3 scripts/check_dependencies.py; then
  : # output already printed by the script
else
  overall_fail=1
fi

# ─────────────────────────────────────────────────────────────────────────────
# 2. Public API purity
# ─────────────────────────────────────────────────────────────────────────────
section "2. Public API purity"
if bash scripts/check_public_api.sh; then
  : # output already printed
else
  overall_fail=1
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3. Vocabulary protection (grep — deprecated aliases must not reappear)
# ─────────────────────────────────────────────────────────────────────────────
section "3. Vocabulary protection"

vocab_fail=0
vocab_check() {
  local label=$1 pattern=$2 path=$3
  if grep -rn "$pattern" "$path" --include="*.rs" -q 2>/dev/null; then
    echo "  FAIL [$label]: pattern '$pattern' found in $path"
    grep -rn "$pattern" "$path" --include="*.rs" | sed 's/^/    /'
    vocab_fail=1
    overall_fail=1
  else
    pass "$label"
  fi
}

vocab_check "Straddle alias not reintroduced"           "pub type Straddle"           cs-domain/src/
vocab_check "ShortIronButterfly alias not reintroduced" "pub type ShortIronButterfly" cs-domain/src/
vocab_check "TradingStrategy trait not reintroduced"    "pub trait TradingStrategy"   cs-domain/src/
vocab_check "StraddleStrategy alias not reintroduced"   "pub type StraddleStrategy"   cs-backtest/src/
vocab_check "no clap:: in cs-domain"                    "clap::\|use clap"            cs-domain/src/
vocab_check "no clap:: in cs-backtest"                  "clap::\|use clap"            cs-backtest/src/

# ─────────────────────────────────────────────────────────────────────────────
# 4. Warning budget — fail if any crate exceeds its baseline
# ─────────────────────────────────────────────────────────────────────────────
section "4. Warning budget"

BASELINE_FILE="scripts/warning_baseline.json"
if ! command -v python3 &>/dev/null; then
  echo "  SKIP: python3 not available for warning budget check"
else
  python3 - "$BASELINE_FILE" <<'EOF'
import json, subprocess, sys, re

baseline_file = sys.argv[1]
with open(baseline_file) as f:
    baseline = json.load(f)

crates = {k: v for k, v in baseline.items() if not k.startswith("_")}
budget_fail = 0

for crate, limit in crates.items():
    result = subprocess.run(
        ["cargo", "check", f"-p{crate}"],
        capture_output=True, text=True
    )
    output = result.stdout + result.stderr
    count = sum(1 for line in output.splitlines() if line.strip().startswith("warning:"))
    status = "OK  " if count <= limit else "FAIL"
    print(f"  {status} {crate}: {count} warning(s) (baseline {limit})")
    if count > limit:
        budget_fail = 1

sys.exit(budget_fail)
EOF
  if [ $? -ne 0 ]; then overall_fail=1; fi
fi

# ─────────────────────────────────────────────────────────────────────────────
echo
if [ "$overall_fail" -eq 0 ]; then
  echo "=== All architecture checks passed ==="
else
  echo "=== FAILED: architecture checks did not pass ==="
fi

exit "$overall_fail"

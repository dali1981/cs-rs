#!/usr/bin/env bash
# Public API purity check.
#
# Parses cs-domain source for `pub` declarations (fn, struct, trait, type, enum)
# and asserts that none of them expose infrastructure types in their signatures.
#
# Forbidden types in public signatures (outside infrastructure/):
#   - polars::*  (DataFrame, Series, LazyFrame, etc.)
#   - DataFrame, LazyFrame, Series  (common polars re-exports)
#
# Usage: bash scripts/check_public_api.sh
# Exit code: 0 = clean, 1 = violations found.

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== Public API purity check (cs-domain) ==="
echo

fail=0

# ─────────────────────────────────────────────────────────────────────────────
# Collect all lines with `pub` declarations in cs-domain, excluding:
#   - infrastructure/ (intentionally uses polars)
#   - test modules (#[cfg(test)] sections are not public API)
# ─────────────────────────────────────────────────────────────────────────────
check_forbidden() {
  local type_label=$1
  local pattern=$2

  # Find pub lines that contain the forbidden pattern, outside infrastructure/
  hits=$(grep -rn "pub\s\+\(fn\|struct\|trait\|type\|enum\)" cs-domain/src/ \
           --include="*.rs" \
           --exclude-dir=infrastructure \
         | grep "$pattern" || true)

  if [ -n "$hits" ]; then
    echo "  FAIL [$type_label in public signature]:"
    echo "$hits" | sed 's/^/    /'
    fail=1
  else
    echo "  OK   [no $type_label in public signatures outside infrastructure/]"
  fi
}

check_forbidden "polars::"    "polars::"
check_forbidden "DataFrame"   "\bDataFrame\b"
check_forbidden "LazyFrame"   "\bLazyFrame\b"
check_forbidden "Series"      "\bSeries\b"

echo
if [ "$fail" -eq 0 ]; then
  echo "=== Result: clean ==="
else
  echo "=== Result: FAILED ==="
fi

exit "$fail"

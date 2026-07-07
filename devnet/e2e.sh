#!/usr/bin/env bash
# P1.M3 e2e suite: scripted round-lifecycle scenarios on the devnet harness.
# Each scenario is an isolated devnet bring-up (own anvil/deploy/keeper/nodes)
# driven by a fixture and asserted by a hook. Exit 0 = all scenarios green.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NODE_REPO="$(cd "$HERE/.." && pwd)"
ONLY="${1:-}"

declare -a NAMES RESULTS
run_scenario() { # name, run.sh args...
  local name=$1; shift
  if [[ -n "$ONLY" && "$name" != *"$ONLY"* ]]; then
    return
  fi
  echo ""
  echo "=============================================================="
  echo "SCENARIO: $name"
  echo "=============================================================="
  if "$HERE/run.sh" "$@"; then
    NAMES+=("$name"); RESULTS+=("PASS")
  else
    NAMES+=("$name"); RESULTS+=("FAIL")
  fi
}

run_scenario "valid claim -> Verified (+emissions, rewards)" \
  --nodes 3 --htxs "$NODE_REPO/data/valid_htx_devnet.json" --hook "$HERE/e2e/verified.sh"

run_scenario "false claim -> Invalid" \
  --nodes 3 --htxs "$NODE_REPO/data/phala_htx.json" --hook "$HERE/e2e/invalid.sh"

run_scenario "inconclusive -> escalation -> expiry (maxEscalations=1)" \
  --nodes 3 --config core.anvil-l1-esc.json \
  --htxs "$NODE_REPO/data/inconclusive_htx_devnet.json" --hook "$HERE/e2e/escalation.sh"

run_scenario "inconclusive -> expiry (maxEscalations=0)" \
  --nodes 3 --htxs "$NODE_REPO/data/inconclusive_htx_devnet.json" --hook "$HERE/e2e/expiry.sh"

run_scenario "non-participant jailed" \
  --nodes 4 --htxs "$NODE_REPO/data/valid_htx_devnet.json" --hook "$HERE/e2e/jailing.sh"

echo ""
echo "==================== E2E SUMMARY ===================="
fail=0
[ "${#NAMES[@]}" -eq 0 ] && { echo "  no scenarios matched '$ONLY'"; exit 2; }
for i in "${!NAMES[@]}"; do
  echo "  ${RESULTS[$i]}  ${NAMES[$i]}"
  [ "${RESULTS[$i]}" = "FAIL" ] && fail=1
done
exit $fail

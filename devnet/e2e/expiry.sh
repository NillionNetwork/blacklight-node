#!/usr/bin/env bash
# Scenario: inconclusive round with maxEscalations=0 -> heartbeat Expired.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check_expired() { [ "$(count_outcome 0)" -ge 1 ] && any_status 4; }
wait_until 300 "an Inconclusive round + an Expired heartbeat" check_expired
echo "expiry: inconclusive rounds=$(count_outcome 0)"

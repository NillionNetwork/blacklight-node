#!/usr/bin/env bash
# Scenario: false claim -> Invalid.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check_invalid() { [ "$(count_outcome 2)" -ge 1 ] && any_status 3; }
wait_until 300 "an InvalidThreshold round + an Invalid heartbeat" check_invalid
echo "invalid: outcome=2 rounds=$(count_outcome 2)"

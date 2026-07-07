#!/usr/bin/env bash
# Scenario: inconclusive round -> escalation (round 2 starts), then expiry at the cap.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check_escalated() { [ "$(count_round_started 2)" -ge 1 ]; }
wait_until 420 "an escalated round 2" check_escalated

check_expired() { any_status 4; }
wait_until 300 "an Expired heartbeat after the escalation cap" check_expired
echo "escalation: round-2 starts=$(count_round_started 2)"

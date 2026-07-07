#!/usr/bin/env bash
# Refresh data/valid_htx_devnet.json (and data/valid_htx.json) from the LIVE nilgpt
# deployment, so the P1.M3 "valid claim -> Verified" scenario keeps passing after
# nilgpt redeploys. Derivation (same as the 2026-07-07 fix):
#   - artifacts_version / cpus / gpus  <- the workload report's environment
#   - docker_compose_hash             <- sha256 of docker-compose-nilcc.yaml @ main
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REPORT_URL="https://nilgpt.xyz/nilcc/api/v2/report"
COMPOSE_URL="https://raw.githubusercontent.com/NillionNetwork/nilgpt/main/docker-compose-nilcc.yaml"

env=$(curl -sf --max-time 20 "$REPORT_URL" | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["environment"]))')
compose_hash=$(curl -sf --max-time 20 "$COMPOSE_URL" | shasum -a 256 | cut -d' ' -f1)

python3 - "$env" "$compose_hash" <<'PYEOF'
import json, sys
env = json.loads(sys.argv[1])
compose_hash = sys.argv[2]

htx = json.load(open("data/valid_htx.json"))
wm = htx["workload_measurement"]
wm["artifacts_version"] = env["nilcc_version"]
wm["cpus"] = env["cpu_count"]
wm["gpus"] = 0 if env.get("vm_type") == "cpu" else 1
wm["docker_compose_hash"] = compose_hash

json.dump(htx, open("data/valid_htx.json", "w"), indent=4)
json.dump([htx], open("data/valid_htx_devnet.json", "w"), indent=2)
print("refreshed:", json.dumps(wm, indent=1))
PYEOF

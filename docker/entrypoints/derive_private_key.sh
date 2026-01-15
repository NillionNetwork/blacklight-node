#!/bin/sh
set -eu

# If the user explicitly provided a PRIVATE_KEY, do nothing.
if [ -n "${PRIVATE_KEY:-}" ]; then
  exec "$@"
fi

if [ -z "${MNEMONIC:-}" ]; then
  echo "ERROR: PRIVATE_KEY not set and MNEMONIC not provided. Set MNEMONIC (and optionally MNEMONIC_BASE_INDEX)." >&2
  exit 1
fi

BASE_INDEX="${MNEMONIC_BASE_INDEX:-0}"
ALLOC_ROOT="${MNEMONIC_ALLOC_ROOT:-/alloc/mnemonic}"
ALLOC_DIR="${ALLOC_ROOT}/allocations"
LOCK_FILE="${ALLOC_ROOT}/lock"
NEXT_FILE="${ALLOC_ROOT}/next"

mkdir -p "${ALLOC_DIR}"

# Use the container hostname as a stable per-container identifier.
# In Docker, this defaults to the container ID and stays stable across restarts of the same container.
ID="${HOSTNAME:-$(cat /etc/hostname 2>/dev/null || echo unknown)}"
ID_FILE="${ALLOC_DIR}/${ID}"

# Allocate a stable mnemonic index for this container using a shared volume.
# - First start: atomically allocates the next index and records it in ${ID_FILE}
# - Restart: reuses the recorded index from ${ID_FILE}
(
  exec 9>"${LOCK_FILE}"
  flock 9

  if [ -f "${ID_FILE}" ]; then
    IDX="$(cat "${ID_FILE}")"
  else
    N="$(cat "${NEXT_FILE}" 2>/dev/null || echo 0)"
    IDX="$((BASE_INDEX + N))"
    echo "$((N + 1))" > "${NEXT_FILE}"
    echo "${IDX}" > "${ID_FILE}"
  fi
) 2>/dev/null

# Read the allocated index (written by the subshell above)
IDX="$(cat "${ID_FILE}")"

# Derive keys via cast
PRIVATE_KEY="$(cast wallet private-key --mnemonic "${MNEMONIC}" --mnemonic-index "${IDX}")"
export PRIVATE_KEY

# Optional: exported for logging/visibility (niluv_node itself only needs PRIVATE_KEY)
PUBLIC_KEY="$(cast wallet address --mnemonic "${MNEMONIC}" --mnemonic-index "${IDX}")"
export PUBLIC_KEY

echo "Derived wallet for container=${ID} mnemonic_index=${IDX} address=${PUBLIC_KEY}" >&2

exec "$@"


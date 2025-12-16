#!/bin/bash
set -e

# Docker entrypoint script that handles permission fixes before dropping privileges
# This script runs as root initially to fix any permission issues, then drops to appuser

# Define the user we'll run as
APP_USER="appuser"
APP_UID=10001
APP_GID=10001

# Fix permissions on directories that need to be writable
# This ensures the application can write to these directories when running as appuser
if [ -d "/app/config" ]; then
    chown -R ${APP_UID}:${APP_GID} /app/config
fi

if [ -d "/app/data" ]; then
    chown -R ${APP_UID}:${APP_GID} /app/data
fi

if [ -d "/app" ]; then
    chown ${APP_UID}:${APP_GID} /app
fi

# Drop privileges and execute the command as appuser
# Use gosu for clean privilege dropping (better than su/sudo in containers)
# If gosu is not available, fall back to su-exec or exec directly
if command -v gosu >/dev/null 2>&1; then
    exec gosu ${APP_USER} "$@"
elif command -v su-exec >/dev/null 2>&1; then
    exec su-exec ${APP_USER} "$@"
else
    # Fallback: just exec the command (will still run as root, but better than nothing)
    exec "$@"
fi

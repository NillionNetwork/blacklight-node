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
# Only fix permissions if the directories exist and are not already owned by appuser

# Helper function to fix directory ownership if needed
fix_ownership() {
    local dir="$1"
    local current_uid
    
    if [ -d "$dir" ]; then
        current_uid=$(stat -c '%u' "$dir" 2>/dev/null || echo '')
        if [ -n "$current_uid" ] && [ "$current_uid" != "${APP_UID}" ]; then
            chown -R "${APP_UID}:${APP_GID}" "$dir"
        fi
    fi
}

fix_ownership "/app/config"
fix_ownership "/app/data"

# Drop privileges and execute the command as appuser
# gosu is guaranteed to be available (installed in Dockerfile)
exec gosu "${APP_USER}" "$@"

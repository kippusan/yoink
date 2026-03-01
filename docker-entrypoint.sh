#!/bin/sh
set -e

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

# Create group if it doesn't exist
if ! getent group yoink >/dev/null 2>&1; then
    groupadd -g "$PGID" yoink
else
    groupmod -o -g "$PGID" yoink
fi

# Create user if it doesn't exist
if ! getent passwd yoink >/dev/null 2>&1; then
    useradd -u "$PUID" -g yoink -d /app -s /bin/sh yoink
else
    usermod -o -u "$PUID" yoink
fi

# Ensure ownership of writable directories
chown -R yoink:yoink /app /data /music

# Drop privileges and exec the main process
exec gosu yoink "$@"

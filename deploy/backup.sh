#!/usr/bin/env bash
# Daily dump of the database into ./backups, keeping the newest 14.
# Copy these files somewhere off the server; a disk on the same machine
# is not a backup.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p backups
file="backups/nycmsa-$(date +%F).dump"
docker compose exec -T db pg_dump -U nycmsa -Fc nycmsa > "$file.tmp"
mv "$file.tmp" "$file"
ls -1t backups/nycmsa-*.dump | tail -n +15 | xargs -r rm --

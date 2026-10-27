#!/usr/bin/env bash
# Pull the newest image and restart only if it changed. Safe to run from
# cron; prints nothing when there is nothing to do.
set -euo pipefail
cd "$(dirname "$0")"
before=$(docker compose images app --format '{{.ID}}' 2>/dev/null || true)
docker compose pull --quiet app init
after=$(docker compose images app --format '{{.ID}}' 2>/dev/null || true)
docker compose up -d --remove-orphans >/dev/null 2>&1
if [ "$before" != "$after" ]; then
  echo "$(date -Is) updated app image $before -> $after"
  docker image prune -f >/dev/null
fi

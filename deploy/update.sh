#!/usr/bin/env bash
# Daily update: refresh the deployment files from the main branch, pull
# the newest image, and restart only what changed. Output goes to the
# cron log; a line is printed only when something changed or failed.
set -euo pipefail
cd "$(dirname "$0")"
raw=https://raw.githubusercontent.com/abenamer/nycmsa/main/deploy
for f in compose.yaml Caddyfile backup.sh update.sh; do
  curl -fsSL "$raw/$f" -o "$f.new"
  if ! cmp -s "$f.new" "$f"; then
    chmod --reference="$f" "$f.new"
    mv "$f.new" "$f"
    echo "$(date -Is) refreshed $f from main"
  else
    rm -f "$f.new"
  fi
done
image=$(docker compose config --format json | python3 -c 'import json,sys; print(json.load(sys.stdin)["services"]["app"]["image"])')
before=$(docker image inspect --format '{{.Id}}' "$image" 2>/dev/null || true)
docker compose pull --quiet app init
after=$(docker image inspect --format '{{.Id}}' "$image")
docker compose up -d --remove-orphans 2>&1 | grep -Ev '^\s*$' | grep -Ev 'Running$|Started$|Healthy$|Exited$' || true
if [ "$before" != "$after" ]; then
  echo "$(date -Is) updated app image ${before:-none} -> $after"
  docker image prune -f >/dev/null
fi

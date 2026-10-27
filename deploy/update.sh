#!/usr/bin/env bash
# Daily update: refresh the deployment files from the main branch, pull
# the newest image, and restart only what changed. Output goes to the
# cron log; a line is printed only when something changed or failed.
#
# The whole script is one function called on the last line, so bash has
# read it completely before it runs and a refreshed update.sh cannot
# change the run in progress.
set -uo pipefail

main() {
  cd "$(dirname "$0")"
  local raw=https://raw.githubusercontent.com/abenamer/nycmsa/main/deploy
  local f
  for f in compose.yaml Caddyfile backup.sh update.sh; do
    if ! curl -fsSL "$raw/$f" -o "$f.new"; then
      echo "$(date -Is) could not fetch $f from main; keeping the current file"
      rm -f "$f.new"
      continue
    fi
    if cmp -s "$f.new" "$f"; then
      rm -f "$f.new"
    else
      chmod --reference="$f" "$f.new"
      mv "$f.new" "$f"
      echo "$(date -Is) refreshed $f from main"
    fi
  done
  local image before after
  image=$(docker compose config --format json | python3 -c 'import json,sys; print(json.load(sys.stdin)["services"]["app"]["image"])') || return 1
  before=$(docker image inspect --format '{{.Id}}' "$image" 2>/dev/null || true)
  docker compose pull --quiet app init || { echo "$(date -Is) image pull failed"; return 1; }
  after=$(docker image inspect --format '{{.Id}}' "$image")
  # `up -d` also starts a stopped `init` again, so a bootstrap that failed
  # earlier is retried here.
  docker compose up -d --remove-orphans 2>&1 | grep -Ev '^\s*$' | grep -Ev 'Running$|Started$|Healthy$|Exited$' || true
  if [ "$before" != "$after" ]; then
    echo "$(date -Is) updated app image ${before:-none} -> $after"
    docker image prune -f >/dev/null
  fi
}

main "$@"

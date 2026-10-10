#!/usr/bin/env bash
# Opens Safari (WebDriver) on a page and runs fetch probes against the local bridge.
# usage: safari-probe.sh <page-url> <probe-url>...
set -u
PAGE=$1; shift
SID=$(curl -s -X POST localhost:4444/session -H 'Content-Type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{"browserName":"safari"}}}' |
  python3 -c 'import sys,json; print(json.load(sys.stdin)["value"]["sessionId"])')
curl -s -X POST "localhost:4444/session/$SID/url" -H 'Content-Type: application/json' -d "{\"url\":\"$PAGE\"}" >/dev/null
sleep 3
for U in "$@"; do
  SCRIPT="const cb=arguments[arguments.length-1]; fetch('$U').then(r=>r.text()).then(t=>cb('OK '+t.slice(0,60))).catch(e=>cb('ERR '+e));"
  BODY=$(python3 -c 'import json,sys; print(json.dumps({"script":sys.argv[1],"args":[]}))' "$SCRIPT")
  RES=$(curl -s -X POST "localhost:4444/session/$SID/execute/async" -H 'Content-Type: application/json' -d "$BODY")
  echo "PROBE page=$PAGE  fetch=$U  ->  $RES"
done
curl -s -X DELETE "localhost:4444/session/$SID" >/dev/null

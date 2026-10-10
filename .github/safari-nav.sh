#!/usr/bin/env bash
# Opens Safari on PAGE, then navigates (like a link click) to TARGET and reports where it landed.
# usage: safari-nav.sh <page-url> <target-url>
set -u
SID=$(curl -s -X POST localhost:4444/session -H 'Content-Type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{"browserName":"safari"}}}' |
  python3 -c 'import sys,json; print(json.load(sys.stdin)["value"]["sessionId"])')
curl -s -X POST "localhost:4444/session/$SID/url" -H 'Content-Type: application/json' -d "{\"url\":\"$1\"}" >/dev/null
sleep 3
BODY=$(python3 -c 'import json,sys; print(json.dumps({"script":"var a=document.createElement(\"a\");a.href=arguments[0];document.body.appendChild(a);a.click();return 1","args":[sys.argv[1]]}))' "$2")
curl -s -X POST "localhost:4444/session/$SID/execute/sync" -H 'Content-Type: application/json' -d "$BODY" >/dev/null
sleep 3
URL=$(curl -s "localhost:4444/session/$SID/url")
TXT=$(curl -s -X POST "localhost:4444/session/$SID/execute/sync" -H 'Content-Type: application/json' -d '{"script":"return location.href+\" secure=\"+window.isSecureContext+\" | \"+document.body.innerText.slice(0,80)","args":[]}')
echo "NAV from=$1 to=$2 -> url=$URL page=$TXT"
curl -s -X DELETE "localhost:4444/session/$SID" >/dev/null

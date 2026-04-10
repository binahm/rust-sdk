#!/usr/bin/env bash

URL="${1:-http://localhost:8000/mcp}"

echo "Connecting to MCP server at $URL"

# POST initialize and capture session ID from response header
SESSION_ID=$(curl -s -D - -o /dev/null \
  -X POST "$URL" \
  -H "Accept: application/json, text/event-stream" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": "2025-06-18",
      "capabilities": {},
      "clientInfo": { "name": "test-client", "version": "1.0.0" }
    }
  }' | grep -i "^mcp-session-id:" | awk '{print $2}' | tr -d '\r')

if [[ -z "$SESSION_ID" ]]; then
  echo "ERROR: No Mcp-Session-Id header in response"
  exit 1
fi

echo "Session ID: $SESSION_ID"
export MCP_SESSION_ID="$SESSION_ID"

# POST notifications/initialized to complete the MCP handshake
STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST "$URL" \
  -H "Accept: application/json, text/event-stream" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $MCP_SESSION_ID" \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -d '{
    "jsonrpc": "2.0",
    "method": "notifications/initialized"
  }')

if [[ "$STATUS" != "202" ]]; then
  echo "ERROR: notifications/initialized returned $STATUS, expected 202" >&2
  exit 1
fi

echo "Handshake complete (202 Accepted)"
echo "MCP_SESSION_ID=$MCP_SESSION_ID"


echo "Calling tool long_task ..."

curl "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  --max-time 5 \
  --data-raw '{"method":"tools/call","params":{"name":"long_task","arguments":{},"_meta":{"progressToken":1}},"jsonrpc":"2.0","id":'"1"'}'

echo "sleeping 3 seconds"
sleep 3
echo "calling resume on the stream after timeout..."

curl "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  -H "last-event-id: 0/0" \
  --max-time 2

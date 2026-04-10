#!/usr/bin/env bash

URL="${1:-http://localhost:3001/mcp}"

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

echo "trigger logging.."
curl "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  --max-time 2 \
  --data-raw '{"method":"logging/setLevel","params":{"level":"debug"},"jsonrpc":"2.0","id":1}'


curl "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  --max-time 2 \
  --data-raw '{"method":"tools/call","params":{"name":"toggle-simulated-logging","arguments":{},"_meta":{"progressToken":2}},"jsonrpc":"2.0","id":2}'

echo "Calling tool long running ..."

EVENT_ID=$(curl "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  --max-time 11 \
  --data-raw '{"method":"tools/call","params":{"name":"trigger-long-running-operation","arguments":{"duration":10,"steps":5},"_meta":{"progressToken":26}},"jsonrpc":"2.0","id":26}' | tee /dev/stderr | grep -im1 "^id:" | awk '{print $2}' | tr -d '\r')

echo "Received last event ID: $EVENT_ID"

echo "sleeping 2 seconds"
sleep 2
echo "calling resume on the stream after 7 seconds timeout..."

curl -v "$URL" \
  -H 'accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -H "Mcp-Protocol-Version: 2025-06-18" \
  --max-time 10 \
  -H "mcp-session-id: $MCP_SESSION_ID" \
  -H "last-event-id: ${EVENT_ID}"

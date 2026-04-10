#!/usr/bin/env bash
set -euo pipefail

URL="${1:-http://localhost:8000/mcp}"
TOOL_NAME="${2:-say_hello}"

echo "Connecting to MCP server at $URL"
echo "Using tool: $TOOL_NAME"

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
  echo "ERROR: No Mcp-Session-Id header in response" >&2
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

for i in {1..2}; do
  echo "Calling tool ... (iteration $i)"
  
  curl -v "$URL" \
    -H 'accept: application/json, text/event-stream' \
    -H 'content-type: application/json' \
    -H "Mcp-Protocol-Version: 2025-06-18" \
    -H "mcp-session-id: $MCP_SESSION_ID" \
    --data-raw '{"method":"tools/call","params":{"name":"'"$TOOL_NAME"'","arguments":{},"_meta":{"progressToken":1}},"jsonrpc":"2.0","id":'"$i"'}'

done

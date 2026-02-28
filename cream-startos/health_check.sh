#!/bin/bash
set -euo pipefail

CONFIG="/data/config.yaml"

# Read port from config, default to 3010
PORT=$(grep -E "^port:" "$CONFIG" 2>/dev/null | sed 's/^port:[[:space:]]*//' | tr -d '"' || echo "")
if [ -z "$PORT" ]; then
    # Default: 3009 + share_index (default share_index=1 → 3010)
    SHARE_INDEX=$(grep -E "^share-index:" "$CONFIG" 2>/dev/null | sed 's/^share-index:[[:space:]]*//' | tr -d '"' || echo "1")
    PORT=$((3009 + SHARE_INDEX))
fi

RESPONSE=$(curl -sf "http://localhost:${PORT}/health" 2>/dev/null) || exit 1

# Check that "ready":true is in the response
echo "$RESPONSE" | grep -q '"ready":true' && exit 0

exit 1

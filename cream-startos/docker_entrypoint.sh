#!/bin/bash
set -euo pipefail

CONFIG="/data/config.yaml"

# Parse YAML config (simple key: value format) into variables
parse_config() {
    local key="$1"
    if [ -f "$CONFIG" ]; then
        grep -E "^${key}:" "$CONFIG" 2>/dev/null | sed "s/^${key}:[[:space:]]*//" | tr -d '"' || echo ""
    else
        echo ""
    fi
}

SHARE_INDEX=$(parse_config "share-index")
PORT=$(parse_config "port")
MAX_SIGNERS=$(parse_config "max-signers")
MIN_SIGNERS=$(parse_config "min-signers")
PEERS=$(parse_config "peers")
NODE_URL=$(parse_config "node-url")

# Build CLI args
ARGS=()

if [ -n "$SHARE_INDEX" ]; then
    ARGS+=(--share-index "$SHARE_INDEX")
fi

if [ -n "$PORT" ]; then
    ARGS+=(--port "$PORT")
fi

if [ -n "$MAX_SIGNERS" ]; then
    ARGS+=(--max-signers "$MAX_SIGNERS")
fi

if [ -n "$MIN_SIGNERS" ]; then
    ARGS+=(--min-signers "$MIN_SIGNERS")
fi

if [ -n "$PEERS" ]; then
    ARGS+=(--peers "$PEERS")
fi

if [ -n "$NODE_URL" ]; then
    ARGS+=(--node-url "$NODE_URL")
fi

# Graceful shutdown on SIGTERM
trap 'kill -TERM $PID; wait $PID' TERM INT

echo "Starting cream-guardian with args: ${ARGS[*]}"
exec cream-guardian "${ARGS[@]}"

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

# Lightning gateway configuration
LIGHTNING_GATEWAY=$(parse_config "lightning-gateway")
LND_HOST=$(parse_config "lnd-host")
LND_PORT=$(parse_config "lnd-port")
LND_CERT=$(parse_config "lnd-cert")
LND_MACAROON=$(parse_config "lnd-macaroon")
PEGIN_LIMIT=$(parse_config "pegin-limit-sats")

if [ "$LIGHTNING_GATEWAY" = "true" ]; then
    ARGS+=(--lightning-gateway)
    # Default LND host on StartOS is lnd.embassy
    ARGS+=(--lnd-host "${LND_HOST:-lnd.embassy}")
    if [ -n "$LND_PORT" ]; then
        ARGS+=(--lnd-port "$LND_PORT")
    fi
    # Default cert/macaroon paths for StartOS LND
    ARGS+=(--lnd-cert "${LND_CERT:-/mnt/lnd/tls.cert}")
    ARGS+=(--lnd-macaroon "${LND_MACAROON:-/mnt/lnd/data/chain/bitcoin/mainnet/admin.macaroon}")
    if [ -n "$PEGIN_LIMIT" ]; then
        ARGS+=(--pegin-limit-sats "$PEGIN_LIMIT")
    fi
fi

# Graceful shutdown on SIGTERM
trap 'kill -TERM $PID; wait $PID' TERM INT

echo "Starting cream-guardian with args: ${ARGS[*]}"
exec cream-guardian "${ARGS[@]}"

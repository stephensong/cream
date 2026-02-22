import { verifyAsync, etc } from '@noble/ed25519';

// @noble/ed25519 v2+ requires explicit SHA-512 — use Web Crypto (available in Workers)
etc.sha512Async = async (...messages: Uint8Array[]) => {
  const combined = etc.concatBytes(...messages);
  const digest = await crypto.subtle.digest('SHA-512', combined);
  return new Uint8Array(digest);
};

interface Env {
  SUPPLIERS: KVNamespace;
}

interface SupplierRecord {
  name: string;
  address: string;
  storefront_key: string;
  public_key: string;
}

interface RegisterBody {
  name: string;
  address: string;
  storefront_key: string;
  public_key: string;
  signature: string;
}

interface HeartbeatBody {
  name: string;
  address: string;
  public_key: string;
  signature: string;
}

const TTL_SECONDS = 7 * 24 * 60 * 60; // 7 days

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function errorResponse(message: string, status: number): Response {
  return jsonResponse({ error: message }, status);
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

async function verifySignature(
  message: string,
  signatureHex: string,
  publicKeyHex: string,
): Promise<boolean> {
  try {
    const msgBytes = new TextEncoder().encode(message);
    const sigBytes = hexToBytes(signatureHex);
    const pubBytes = hexToBytes(publicKeyHex);
    return await verifyAsync(sigBytes, msgBytes, pubBytes);
  } catch {
    return false;
  }
}

async function handleRegister(request: Request, env: Env): Promise<Response> {
  let body: RegisterBody;
  try {
    body = await request.json() as RegisterBody;
  } catch {
    return errorResponse('Invalid JSON', 400);
  }

  const { name, address, storefront_key, public_key, signature } = body;
  if (!name || !address || !storefront_key || !public_key || !signature) {
    return errorResponse('Missing required fields', 400);
  }

  const normalizedName = name.toLowerCase();

  // Verify signature over "name|address|storefront_key"
  const message = `${normalizedName}|${address}|${storefront_key}`;
  const valid = await verifySignature(message, signature, public_key);
  if (!valid) {
    return errorResponse('Invalid signature', 400);
  }

  // Check if name is taken by a different key
  const existing = await env.SUPPLIERS.get(normalizedName, 'json') as SupplierRecord | null;
  if (existing && existing.public_key !== public_key) {
    return errorResponse('Name taken by a different key', 409);
  }

  const record: SupplierRecord = {
    name: normalizedName,
    address,
    storefront_key,
    public_key,
  };

  await env.SUPPLIERS.put(normalizedName, JSON.stringify(record), {
    expirationTtl: TTL_SECONDS,
  });

  return jsonResponse({ ok: true });
}

async function handleHeartbeat(request: Request, env: Env): Promise<Response> {
  let body: HeartbeatBody;
  try {
    body = await request.json() as HeartbeatBody;
  } catch {
    return errorResponse('Invalid JSON', 400);
  }

  const { name, address, public_key, signature } = body;
  if (!name || !address || !public_key || !signature) {
    return errorResponse('Missing required fields', 400);
  }

  const normalizedName = name.toLowerCase();

  const existing = await env.SUPPLIERS.get(normalizedName, 'json') as SupplierRecord | null;
  if (!existing) {
    return errorResponse('Supplier not found', 404);
  }

  if (existing.public_key !== public_key) {
    return errorResponse('Key mismatch', 403);
  }

  // Verify signature over "name|address"
  const message = `${normalizedName}|${address}`;
  const valid = await verifySignature(message, signature, public_key);
  if (!valid) {
    return errorResponse('Invalid signature', 400);
  }

  // Update address and refresh TTL
  const record: SupplierRecord = {
    ...existing,
    address,
  };

  await env.SUPPLIERS.put(normalizedName, JSON.stringify(record), {
    expirationTtl: TTL_SECONDS,
  });

  return jsonResponse({ ok: true });
}

async function handleLookup(name: string, env: Env): Promise<Response> {
  const normalizedName = name.toLowerCase();
  const record = await env.SUPPLIERS.get(normalizedName, 'json') as SupplierRecord | null;

  if (!record) {
    return errorResponse('Supplier not found', 404);
  }

  return jsonResponse({
    name: record.name,
    address: record.address,
    storefront_key: record.storefront_key,
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    // CORS headers for browser requests
    if (request.method === 'OPTIONS') {
      return new Response(null, {
        headers: {
          'Access-Control-Allow-Origin': '*',
          'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
          'Access-Control-Allow-Headers': 'Content-Type',
        },
      });
    }

    let response: Response;

    if (request.method === 'POST' && path === '/register') {
      response = await handleRegister(request, env);
    } else if (request.method === 'POST' && path === '/heartbeat') {
      response = await handleHeartbeat(request, env);
    } else if (request.method === 'GET' && path.startsWith('/lookup/')) {
      const name = path.slice('/lookup/'.length);
      if (!name) {
        response = errorResponse('Name required', 400);
      } else {
        response = await handleLookup(decodeURIComponent(name), env);
      }
    } else {
      response = errorResponse('Not found', 404);
    }

    // Add CORS header to all responses
    response.headers.set('Access-Control-Allow-Origin', '*');
    return response;
  },
};

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { WebSocket } from 'ws';
import { getPublicKeyAsync, signAsync, utils } from '@noble/ed25519';
import { etc } from '@noble/ed25519';
import { createRelay, type RelayServer } from '../index.js';

// Configure SHA-512 for @noble/ed25519 v2+
etc.sha512Async = async (...messages: Uint8Array[]) => {
  const { createHash } = await import('node:crypto');
  const hash = createHash('sha512');
  for (const msg of messages) {
    hash.update(msg);
  }
  return new Uint8Array(hash.digest());
};

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

async function makeKeypair() {
  const privKey = utils.randomPrivateKey();
  const pubKey = await getPublicKeyAsync(privKey);
  return { privKey, pubKey: bytesToHex(pubKey) };
}

async function signMessage(privKey: Uint8Array, message: string): Promise<string> {
  const msgBytes = new TextEncoder().encode(message);
  const sig = await signAsync(msgBytes, privKey);
  return bytesToHex(sig);
}

// Helper: connect and wait for nonce
function connectClient(port: number): Promise<{ ws: WebSocket; nonce: string }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://localhost:${port}`);
    ws.on('error', reject);
    ws.on('message', (data) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === 'nonce') {
        resolve({ ws, nonce: msg.nonce });
      }
    });
  });
}

// Helper: authenticate a client
async function authenticateClient(
  port: number,
  keypair: { privKey: Uint8Array; pubKey: string },
): Promise<WebSocket> {
  const { ws, nonce } = await connectClient(port);
  const signature = await signMessage(keypair.privKey, nonce);
  ws.send(JSON.stringify({ type: 'auth', public_key: keypair.pubKey, signature, nonce }));
  await waitForMessage(ws, 'auth_ok');
  return ws;
}

// Helper: wait for a specific message type
function waitForMessage(ws: WebSocket, type: string, timeoutMs = 3000): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`Timeout waiting for ${type}`)), timeoutMs);
    const handler = (data: Buffer) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === type) {
        clearTimeout(timer);
        ws.off('message', handler);
        resolve(msg);
      }
    };
    ws.on('message', handler);
  });
}

// Helper: close WebSocket and wait for close event
function closeWs(ws: WebSocket): Promise<void> {
  return new Promise((resolve) => {
    if (ws.readyState === WebSocket.CLOSED) {
      resolve();
      return;
    }
    ws.on('close', () => resolve());
    ws.close();
  });
}

const TEST_PORT = 13020; // Use a non-standard port to avoid conflicts
let server: RelayServer;

beforeEach(async () => {
  server = await createRelay(TEST_PORT);
});

afterEach(async () => {
  // Close all connected clients
  for (const ws of server.clients.keys()) {
    ws.close();
  }
  await new Promise<void>((resolve) => {
    server.wss.close(() => resolve());
  });
});

describe('Authentication', () => {
  it('sends nonce on connect', async () => {
    const { ws, nonce } = await connectClient(TEST_PORT);
    expect(nonce).toBeTruthy();
    expect(typeof nonce).toBe('string');
    expect(nonce.length).toBe(36); // UUID format
    await closeWs(ws);
  });

  it('accepts valid auth', async () => {
    const kp = await makeKeypair();
    const ws = await authenticateClient(TEST_PORT, kp);
    expect(server.pubkeyToSocket.has(kp.pubKey)).toBe(true);
    await closeWs(ws);
  });

  it('rejects invalid signature', async () => {
    const { ws, nonce } = await connectClient(TEST_PORT);
    const kp = await makeKeypair();
    const badSig = '00'.repeat(64); // 128 hex chars but invalid sig
    ws.send(JSON.stringify({ type: 'auth', public_key: kp.pubKey, signature: badSig, nonce }));
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Invalid signature');
    await closeWs(ws);
  });

  it('rejects wrong nonce', async () => {
    const { ws } = await connectClient(TEST_PORT);
    const kp = await makeKeypair();
    const signature = await signMessage(kp.privKey, 'wrong-nonce');
    ws.send(JSON.stringify({ type: 'auth', public_key: kp.pubKey, signature, nonce: 'wrong-nonce' }));
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Nonce mismatch');
    await closeWs(ws);
  });

  it('rejects double auth', async () => {
    const kp = await makeKeypair();
    const ws = await authenticateClient(TEST_PORT, kp);
    ws.send(JSON.stringify({ type: 'auth', public_key: kp.pubKey, signature: '00'.repeat(64), nonce: 'x' }));
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Already authenticated');
    await closeWs(ws);
  });

  it('requires auth before sending messages', async () => {
    const { ws } = await connectClient(TEST_PORT);
    ws.send(JSON.stringify({ type: 'text', session_id: 'x', ciphertext: 'hi', nonce: 'n' }));
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Not authenticated');
    await closeWs(ws);
  });

  it('replaces existing connection for same pubkey', async () => {
    const kp = await makeKeypair();
    const ws1 = await authenticateClient(TEST_PORT, kp);

    // Connect second time with same key — ws1 should get an error
    const errorPromise = waitForMessage(ws1, 'error');
    const ws2 = await authenticateClient(TEST_PORT, kp);
    const errorMsg = await errorPromise;

    expect(errorMsg.message).toBe('Replaced by new connection');
    // The map should now have exactly one entry for this pubkey
    expect(server.pubkeyToSocket.has(kp.pubKey)).toBe(true);

    await closeWs(ws1);
    await closeWs(ws2);
  });
});

describe('Invites', () => {
  it('delivers invite to connected peer', async () => {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    const invitePromise = waitForMessage(wsB, 'invite');
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'test-session-1',
      ecdh_pubkey: '00'.repeat(32),
    }));

    const invite = await invitePromise;
    expect(invite.from).toBe(alice.pubKey);
    expect(invite.session_id).toBe('test-session-1');
    expect(invite.ecdh_pubkey).toBe('00'.repeat(32));

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('fails when peer is not connected', async () => {
    const alice = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);

    wsA.send(JSON.stringify({
      type: 'invite',
      to: '00'.repeat(32),
      session_id: 'test-session-2',
      ecdh_pubkey: '00'.repeat(32),
    }));

    const msg = await waitForMessage(wsA, 'error');
    expect(msg.message).toBe('Peer not connected');
    await closeWs(wsA);
  });

  it('rejects duplicate session ID', async () => {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'dup-session',
      ecdh_pubkey: '00'.repeat(32),
    }));
    await waitForMessage(wsB, 'invite');

    // Try same session ID again
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'dup-session',
      ecdh_pubkey: '00'.repeat(32),
    }));
    const msg = await waitForMessage(wsA, 'error');
    expect(msg.message).toBe('Session ID already exists');

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('rejects self-invite', async () => {
    const alice = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);

    wsA.send(JSON.stringify({
      type: 'invite',
      to: alice.pubKey,
      session_id: 'self-session',
      ecdh_pubkey: '00'.repeat(32),
    }));

    const msg = await waitForMessage(wsA, 'error');
    expect(msg.message).toBe('Cannot invite yourself');
    await closeWs(wsA);
  });
});

describe('Session messaging', () => {
  async function setupSession() {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    // Alice invites Bob
    const invitePromise = waitForMessage(wsB, 'invite');
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'chat-session',
      ecdh_pubkey: 'aa'.repeat(32),
    }));
    await invitePromise;

    // Bob accepts
    const acceptPromise = waitForMessage(wsA, 'accept');
    wsB.send(JSON.stringify({
      type: 'accept',
      session_id: 'chat-session',
      ecdh_pubkey: 'bb'.repeat(32),
    }));
    const accept = await acceptPromise;
    expect(accept.ecdh_pubkey).toBe('bb'.repeat(32));

    return { alice, bob, wsA, wsB };
  }

  it('relays text messages', async () => {
    const { wsA, wsB } = await setupSession();

    const textPromise = waitForMessage(wsB, 'text');
    wsA.send(JSON.stringify({
      type: 'text',
      session_id: 'chat-session',
      ciphertext: 'aGVsbG8=',
      nonce: 'bm9uY2U=',
    }));

    const text = await textPromise;
    expect(text.ciphertext).toBe('aGVsbG8=');
    expect(text.nonce).toBe('bm9uY2U=');

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('relays SDP offers/answers', async () => {
    const { wsA, wsB } = await setupSession();

    const sdpPromise = waitForMessage(wsB, 'sdp');
    wsA.send(JSON.stringify({
      type: 'sdp',
      session_id: 'chat-session',
      sdp: { type: 'offer', sdp: 'v=0...' },
    }));

    const sdp = await sdpPromise;
    expect((sdp.sdp as Record<string, unknown>).type).toBe('offer');

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('relays ICE candidates', async () => {
    const { wsA, wsB } = await setupSession();

    const icePromise = waitForMessage(wsB, 'ice');
    wsA.send(JSON.stringify({
      type: 'ice',
      session_id: 'chat-session',
      candidate: { candidate: 'candidate:1...', sdpMid: '0' },
    }));

    const ice = await icePromise;
    expect((ice.candidate as Record<string, unknown>).sdpMid).toBe('0');

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('rejects non-participants', async () => {
    await setupSession();
    const charlie = await makeKeypair();
    const wsC = await authenticateClient(TEST_PORT, charlie);

    wsC.send(JSON.stringify({
      type: 'text',
      session_id: 'chat-session',
      ciphertext: 'hack',
      nonce: 'x',
    }));

    const msg = await waitForMessage(wsC, 'error');
    expect(msg.message).toBe('Not a participant in this session');
    await closeWs(wsC);
  });
});

describe('Session lifecycle', () => {
  it('close destroys session and notifies peer', async () => {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    // Create session
    const invitePromise = waitForMessage(wsB, 'invite');
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'close-test',
      ecdh_pubkey: '00'.repeat(32),
    }));
    await invitePromise;

    wsB.send(JSON.stringify({ type: 'accept', session_id: 'close-test', ecdh_pubkey: '00'.repeat(32) }));
    await waitForMessage(wsA, 'accept');

    // Alice closes
    const closePromise = waitForMessage(wsB, 'close');
    wsA.send(JSON.stringify({ type: 'close', session_id: 'close-test' }));
    const closeMsg = await closePromise;
    expect(closeMsg.reason).toBe('closed_by_peer');

    // Session should be gone
    expect(server.sessions.get('close-test')).toBeUndefined();

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('decline destroys session and notifies inviter', async () => {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    const invitePromise = waitForMessage(wsB, 'invite');
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'decline-test',
      ecdh_pubkey: '00'.repeat(32),
    }));
    await invitePromise;

    const declinePromise = waitForMessage(wsA, 'decline');
    wsB.send(JSON.stringify({ type: 'decline', session_id: 'decline-test' }));
    await declinePromise;

    expect(server.sessions.get('decline-test')).toBeUndefined();

    await closeWs(wsA);
    await closeWs(wsB);
  });

  it('disconnect destroys sessions and notifies peers', async () => {
    const alice = await makeKeypair();
    const bob = await makeKeypair();
    const wsA = await authenticateClient(TEST_PORT, alice);
    const wsB = await authenticateClient(TEST_PORT, bob);

    // Create session
    const invitePromise = waitForMessage(wsB, 'invite');
    wsA.send(JSON.stringify({
      type: 'invite',
      to: bob.pubKey,
      session_id: 'dc-test',
      ecdh_pubkey: '00'.repeat(32),
    }));
    await invitePromise;

    wsB.send(JSON.stringify({ type: 'accept', session_id: 'dc-test', ecdh_pubkey: '00'.repeat(32) }));
    await waitForMessage(wsA, 'accept');

    // Alice disconnects abruptly
    const closePromise = waitForMessage(wsB, 'close');
    wsA.close();
    const closeMsg = await closePromise;
    expect(closeMsg.reason).toBe('peer_disconnected');
    expect(server.sessions.get('dc-test')).toBeUndefined();

    await closeWs(wsB);
  });
});

describe('Error handling', () => {
  it('rejects invalid JSON', async () => {
    const kp = await makeKeypair();
    const ws = await authenticateClient(TEST_PORT, kp);
    ws.send('not json');
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Invalid JSON');
    await closeWs(ws);
  });

  it('rejects unknown message type', async () => {
    const kp = await makeKeypair();
    const ws = await authenticateClient(TEST_PORT, kp);
    ws.send(JSON.stringify({ type: 'unknown' }));
    const msg = await waitForMessage(ws, 'error');
    expect(msg.message).toBe('Unknown message type');
    await closeWs(ws);
  });
});

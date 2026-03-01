import { WebSocketServer, WebSocket } from 'ws';
import { generateNonce, verifyAuth } from './auth.js';
import { SessionManager } from './session.js';
import type { AuthenticatedClient, ClientMessage, ServerMessage } from './types.js';

const DEFAULT_PORT = 3020;

export interface RelayServer {
  wss: WebSocketServer;
  sessions: SessionManager;
  clients: Map<WebSocket, AuthenticatedClient>;
  pubkeyToSocket: Map<string, WebSocket>;
  close: () => void;
}

export function createRelay(port: number = DEFAULT_PORT): Promise<RelayServer> {
  return new Promise((resolve) => {
    const wss = new WebSocketServer({ port });
    const sessions = new SessionManager();
    const clients = new Map<WebSocket, AuthenticatedClient>();
    const pubkeyToSocket = new Map<string, WebSocket>();

    function send(ws: WebSocket, msg: ServerMessage): void {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(msg));
      }
    }

    function sendToPubkey(pubkey: string, msg: ServerMessage): boolean {
      const ws = pubkeyToSocket.get(pubkey);
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(msg));
        return true;
      }
      return false;
    }

    function destroySession(sessionId: string, reason: string): void {
      const session = sessions.destroy(sessionId);
      if (session) {
        for (const pubkey of session.participants) {
          sendToPubkey(pubkey, { type: 'close', session_id: sessionId, reason });
        }
      }
    }

    function cleanupClient(ws: WebSocket): void {
      const client = clients.get(ws);
      if (client?.authenticated) {
        // Close all sessions this client participates in
        const clientSessions = sessions.getSessionsForPubkey(client.pubkey);
        for (const session of clientSessions) {
          destroySession(session.id, 'peer_disconnected');
        }
        pubkeyToSocket.delete(client.pubkey);
      }
      clients.delete(ws);
    }

    wss.on('connection', (ws: WebSocket) => {
      const nonce = generateNonce();
      const clientState: AuthenticatedClient = {
        pubkey: '',
        nonce,
        authenticated: false,
      };
      clients.set(ws, clientState);

      // Send nonce challenge
      send(ws, { type: 'nonce', nonce });

      ws.on('message', async (data: Buffer) => {
        let msg: ClientMessage;
        try {
          msg = JSON.parse(data.toString()) as ClientMessage;
        } catch {
          send(ws, { type: 'error', message: 'Invalid JSON' });
          return;
        }

        const client = clients.get(ws)!;

        // Handle auth
        if (msg.type === 'auth') {
          if (client.authenticated) {
            send(ws, { type: 'error', message: 'Already authenticated' });
            return;
          }

          if (msg.nonce !== client.nonce) {
            send(ws, { type: 'error', message: 'Nonce mismatch' });
            return;
          }

          const valid = await verifyAuth(msg.nonce, msg.signature, msg.public_key);
          if (!valid) {
            send(ws, { type: 'error', message: 'Invalid signature' });
            return;
          }

          // Disconnect any existing connection with this pubkey
          const existing = pubkeyToSocket.get(msg.public_key);
          if (existing && existing !== ws) {
            send(existing, { type: 'error', message: 'Replaced by new connection' });
            existing.close();
          }

          client.pubkey = msg.public_key;
          client.authenticated = true;
          pubkeyToSocket.set(msg.public_key, ws);
          send(ws, { type: 'auth_ok' });
          return;
        }

        // All other messages require authentication
        if (!client.authenticated) {
          send(ws, { type: 'error', message: 'Not authenticated' });
          return;
        }

        switch (msg.type) {
          case 'invite': {
            if (msg.to === client.pubkey) {
              send(ws, { type: 'error', message: 'Cannot invite yourself' });
              return;
            }

            const session = sessions.create(msg.session_id, client.pubkey, msg.to, (sid) => {
              destroySession(sid, 'session_expired');
            });

            if (!session) {
              send(ws, { type: 'error', message: 'Session ID already exists' });
              return;
            }

            const delivered = sendToPubkey(msg.to, {
              type: 'invite',
              from: client.pubkey,
              session_id: msg.session_id,
              ecdh_pubkey: msg.ecdh_pubkey,
            });

            if (!delivered) {
              sessions.destroy(msg.session_id);
              send(ws, { type: 'error', message: 'Peer not connected' });
            }
            break;
          }

          case 'accept': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }

            const other = sessions.getOtherParticipant(msg.session_id, client.pubkey);
            if (other) {
              sendToPubkey(other, {
                type: 'accept',
                session_id: msg.session_id,
                ecdh_pubkey: msg.ecdh_pubkey,
              });
            }
            break;
          }

          case 'decline': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }

            const other = sessions.getOtherParticipant(msg.session_id, client.pubkey);
            if (other) {
              sendToPubkey(other, { type: 'decline', session_id: msg.session_id });
            }
            sessions.destroy(msg.session_id);
            break;
          }

          case 'text': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }

            const other = sessions.getOtherParticipant(msg.session_id, client.pubkey);
            if (other) {
              sendToPubkey(other, {
                type: 'text',
                session_id: msg.session_id,
                ciphertext: msg.ciphertext,
                nonce: msg.nonce,
              });
            }
            break;
          }

          case 'sdp': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }

            const other = sessions.getOtherParticipant(msg.session_id, client.pubkey);
            if (other) {
              sendToPubkey(other, {
                type: 'sdp',
                session_id: msg.session_id,
                sdp: msg.sdp,
              });
            }
            break;
          }

          case 'ice': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }

            const other = sessions.getOtherParticipant(msg.session_id, client.pubkey);
            if (other) {
              sendToPubkey(other, {
                type: 'ice',
                session_id: msg.session_id,
                candidate: msg.candidate,
              });
            }
            break;
          }

          case 'close': {
            if (!sessions.isParticipant(msg.session_id, client.pubkey)) {
              send(ws, { type: 'error', message: 'Not a participant in this session' });
              return;
            }
            destroySession(msg.session_id, 'closed_by_peer');
            break;
          }

          default:
            send(ws, { type: 'error', message: 'Unknown message type' });
        }
      });

      ws.on('close', () => {
        cleanupClient(ws);
      });

      ws.on('error', () => {
        cleanupClient(ws);
      });
    });

    wss.on('listening', () => {
      resolve({ wss, sessions, clients, pubkeyToSocket, close: () => wss.close() });
    });
  });
}

// CLI entry point
const isMainModule = process.argv[1] && (
  process.argv[1].endsWith('/index.ts') ||
  process.argv[1].endsWith('/index.js')
);

if (isMainModule) {
  const port = parseInt(process.argv.find((a) => a.startsWith('--port='))?.split('=')[1] ?? '', 10) || DEFAULT_PORT;
  createRelay(port).then(() => {
    console.log(`CREAM relay listening on port ${port}`);
  });
}

/** Messages sent by the server to clients */
export type ServerMessage =
  | { type: 'nonce'; nonce: string }
  | { type: 'auth_ok' }
  | { type: 'error'; message: string }
  | { type: 'invite'; from: string; session_id: string; ecdh_pubkey: string }
  | { type: 'accept'; session_id: string; ecdh_pubkey: string }
  | { type: 'decline'; session_id: string }
  | { type: 'text'; session_id: string; ciphertext: string; nonce: string }
  | { type: 'sdp'; session_id: string; sdp: unknown }
  | { type: 'ice'; session_id: string; candidate: unknown }
  | { type: 'close'; session_id: string; reason: string };

/** Messages sent by clients to the server */
export type ClientMessage =
  | { type: 'auth'; public_key: string; signature: string; nonce: string }
  | { type: 'invite'; to: string; session_id: string; ecdh_pubkey: string }
  | { type: 'accept'; session_id: string; ecdh_pubkey: string }
  | { type: 'decline'; session_id: string }
  | { type: 'text'; session_id: string; ciphertext: string; nonce: string }
  | { type: 'sdp'; session_id: string; sdp: unknown }
  | { type: 'ice'; session_id: string; candidate: unknown }
  | { type: 'close'; session_id: string };

export interface Session {
  id: string;
  participants: [string, string]; // two public key hex strings
  createdAt: number;
  timer: ReturnType<typeof setTimeout>;
}

export interface AuthenticatedClient {
  pubkey: string;
  nonce: string;
  authenticated: boolean;
}

import { verifyAsync, etc } from '@noble/ed25519';
import { v4 as uuidv4 } from 'uuid';

// @noble/ed25519 v2+ requires explicit SHA-512
etc.sha512Async = async (...messages: Uint8Array[]) => {
  const { createHash } = await import('node:crypto');
  const hash = createHash('sha512');
  for (const msg of messages) {
    hash.update(msg);
  }
  return new Uint8Array(hash.digest());
};

export function generateNonce(): string {
  return uuidv4();
}

export function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

/**
 * Verify an ed25519 signature over a nonce string.
 * The client signs the raw nonce bytes (UTF-8 encoded).
 */
export async function verifyAuth(
  nonce: string,
  signatureHex: string,
  publicKeyHex: string,
): Promise<boolean> {
  try {
    if (publicKeyHex.length !== 64 || signatureHex.length !== 128) {
      return false;
    }
    const msgBytes = new TextEncoder().encode(nonce);
    const sigBytes = hexToBytes(signatureHex);
    const pubBytes = hexToBytes(publicKeyHex);
    return await verifyAsync(sigBytes, msgBytes, pubBytes);
  } catch {
    return false;
  }
}

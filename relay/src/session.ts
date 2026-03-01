import type { Session } from './types.js';

const MAX_SESSION_DURATION_MS = 60 * 60 * 1000; // 60 minutes

export class SessionManager {
  private sessions: Map<string, Session> = new Map();

  /** Create a new session between two participants. Returns the session or null if either is already in this session. */
  create(sessionId: string, initiator: string, target: string, onExpire: (sessionId: string) => void): Session | null {
    if (this.sessions.has(sessionId)) {
      return null;
    }

    const timer = setTimeout(() => {
      onExpire(sessionId);
    }, MAX_SESSION_DURATION_MS);

    const session: Session = {
      id: sessionId,
      participants: [initiator, target],
      createdAt: Date.now(),
      timer,
    };

    this.sessions.set(sessionId, session);
    return session;
  }

  /** Get a session by ID */
  get(sessionId: string): Session | undefined {
    return this.sessions.get(sessionId);
  }

  /** Check if a pubkey is a participant in the given session */
  isParticipant(sessionId: string, pubkey: string): boolean {
    const session = this.sessions.get(sessionId);
    if (!session) return false;
    return session.participants.includes(pubkey);
  }

  /** Get the other participant in a session */
  getOtherParticipant(sessionId: string, pubkey: string): string | null {
    const session = this.sessions.get(sessionId);
    if (!session) return null;
    const other = session.participants.find((p) => p !== pubkey);
    return other ?? null;
  }

  /** Destroy a session and clear its timer */
  destroy(sessionId: string): Session | undefined {
    const session = this.sessions.get(sessionId);
    if (session) {
      clearTimeout(session.timer);
      this.sessions.delete(sessionId);
    }
    return session;
  }

  /** Get all sessions a pubkey participates in */
  getSessionsForPubkey(pubkey: string): Session[] {
    const result: Session[] = [];
    for (const session of this.sessions.values()) {
      if (session.participants.includes(pubkey)) {
        result.push(session);
      }
    }
    return result;
  }

  /** Get the number of active sessions */
  get size(): number {
    return this.sessions.size;
  }
}

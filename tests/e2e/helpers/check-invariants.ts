import { execSync } from 'child_process';
import * as path from 'path';

/** Path to the check-invariants binary (release build). */
const BINARY = path.resolve(__dirname, '../../../target/release/check-invariants');

/**
 * Run the CURD conservation invariant check.
 *
 * Connects to the Freenet node, GETs all user contracts, and verifies:
 * 1. Each contract's cached balance matches its ledger replay
 * 2. Total CURD across all contracts == 1,000,000 (SYSTEM_FLOAT)
 *
 * Throws if the check fails.
 */
export function checkInvariants(label: string): void {
  try {
    const output = execSync(`"${BINARY}" "${label}" --soft`, {
      timeout: 120_000,
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    if (output.trim()) {
      console.log(output.trim());
    }
  } catch (err: any) {
    const stderr = err.stderr?.toString() || '';
    const stdout = err.stdout?.toString() || '';
    throw new Error(
      `CURD invariant check failed at "${label}":\n${stdout}\n${stderr}`
    );
  }
}

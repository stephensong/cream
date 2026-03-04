import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { dismissDxToast } from '../helpers/wait-for-app';

/**
 * Revoke a user's admin status via guardian HTTP API (idempotent cleanup).
 * Silently ignores errors (user may not be admin).
 */
async function revokeAdmin(rootPubkey: string, userPubkey: string): Promise<void> {
  for (const port of [3010, 3011, 3012]) {
    try {
      await fetch(`http://localhost:${port}/admin-revoke`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ grantor: rootPubkey, pubkey: userPubkey }),
      });
    } catch {
      // Guardian may be down, ignore
    }
  }
}

test.describe('Admin Grant', () => {
  test('root grants admin to non-admin user, Admin button appears after refresh', async ({ browser }) => {
    // Two independent browser contexts = two separate identities
    const rootCtx = await browser.newContext();
    const userCtx = await browser.newContext();
    const rootPage = await rootCtx.newPage();
    const userPage = await userCtx.newPage();

    // ── Step 1: Log in as "root" (pre-seeded admin via --admin-pubkeys) ──
    await completeSetup(rootPage, {
      name: 'root',
      postcode: '2000',
    });

    // Root should see the "Root" nav button (admin check is async, may need reload)
    try {
      await expect(rootPage.locator('button:has-text("Root")')).toBeVisible({ timeout: 10_000 });
    } catch {
      // Admin check may not have completed before initial render; reload to re-trigger
      await rootPage.reload();
      await expect(rootPage.locator('.app-header')).toBeVisible({ timeout: 15_000 });
      await dismissDxToast(rootPage);
      await expect(rootPage.locator('button:has-text("Root")')).toBeVisible({ timeout: 15_000 });
    }

    // Get root's pubkey for API calls
    await rootPage.click('.user-moniker.clickable');
    await expect(rootPage.locator('.profile-pubkey')).toBeVisible({ timeout: 10_000 });
    const rootPubkey = (await rootPage.locator('.profile-pubkey').textContent())!.trim();
    // Navigate back from profile
    await rootPage.click('button:has-text("Browse Suppliers")');

    // ── Step 2: Log in as Alice (existing harness user) ──
    await completeSetup(userPage, {
      name: 'Alice',
      postcode: '2000',
    });

    // Get Alice's pubkey from profile
    await userPage.click('.user-moniker.clickable');
    await expect(userPage.locator('.profile-pubkey')).toBeVisible({ timeout: 10_000 });
    const userPubkey = (await userPage.locator('.profile-pubkey').textContent())!.trim();
    expect(userPubkey.length).toBeGreaterThan(10);

    // Revoke Alice's admin if left over from a prior run (idempotent cleanup)
    await revokeAdmin(rootPubkey, userPubkey);

    // Reload Alice to pick up revocation
    await userPage.reload();
    await expect(userPage.locator('.app-header')).toBeVisible({ timeout: 15_000 });
    await dismissDxToast(userPage);

    // Alice should NOT see Root or Admin button
    await expect(userPage.locator('button:has-text("Root")')).not.toBeVisible();
    await expect(userPage.locator('button:has-text("Admin")')).not.toBeVisible();

    // ── Step 3: Root navigates to Guardian Admin and grants admin to Alice ──
    await rootPage.click('button:has-text("Root")');
    await expect(rootPage.locator('.guardian-admin')).toBeVisible({ timeout: 10_000 });

    // Wait for the "Manage Admins" section to load
    await expect(rootPage.locator('h3:has-text("Manage Admins")')).toBeVisible({ timeout: 10_000 });

    // Fill in Alice's pubkey and grant admin
    await rootPage.fill('input[placeholder="User pubkey (hex)"]', userPubkey);
    await rootPage.click('button:has-text("Grant Admin")');

    // Wait for success feedback
    await expect(rootPage.locator('.alert-success:has-text("Admin granted")')).toBeVisible({ timeout: 10_000 });

    // Verify the admin list now has 2 entries (root + Alice)
    const adminRows = rootPage.locator('.guardian-admin table tbody tr');
    await expect(adminRows).toHaveCount(2, { timeout: 10_000 });

    // ── Step 4: Alice refreshes and sees Admin button ──
    await userPage.reload();
    await expect(userPage.locator('.app-header')).toBeVisible({ timeout: 15_000 });
    await dismissDxToast(userPage);

    // Alice should now see "Admin" (not "Root" — she's a non-root admin)
    await expect(userPage.locator('button:has-text("Admin")')).toBeVisible({ timeout: 15_000 });
    await expect(userPage.locator('button:has-text("Root")')).not.toBeVisible();

    // ── Step 5: Alice can access Guardian Admin but not root-only sections ──
    await userPage.click('button:has-text("Admin")');
    await expect(userPage.locator('.guardian-admin')).toBeVisible({ timeout: 10_000 });
    await expect(userPage.locator('h2:has-text("Guardian Admin")')).toBeVisible();

    // Non-root admin should NOT see Toll Rates editor or Manage Admins section
    await expect(userPage.locator('h3:has-text("Toll Rates")')).not.toBeVisible();
    await expect(userPage.locator('h3:has-text("Manage Admins")')).not.toBeVisible();

    // ── Cleanup: revoke Alice's admin so the test is idempotent ──
    await revokeAdmin(rootPubkey, userPubkey);

    await rootCtx.close();
    await userCtx.close();
  });
});

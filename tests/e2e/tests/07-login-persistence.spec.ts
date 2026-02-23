import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForAppLoad } from '../helpers/wait-for-app';

test.describe('Login Persistence', () => {
  test('page reload preserves session from sessionStorage', async ({ page }) => {
    await completeSetup(page, {
      name: 'Persistent',
      postcode: '4000',
      isSupplier: true,
      description: 'Testing persistence',
    });

    // Verify we're logged in
    await expect(page.locator('.user-moniker')).toHaveText('Persistent');
    await expect(page.locator('.role-badge')).toBeVisible();

    // Reload the page
    await page.reload();

    // Wait for WASM to reload
    await expect(
      page.locator('.user-setup, .app-header').first()
    ).toBeVisible({ timeout: 30_000 });

    // Should auto-login from sessionStorage — header should appear, not setup screen
    await expect(page.locator('.app-header')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.user-moniker')).toHaveText('Persistent');
    await expect(page.locator('.role-badge')).toBeVisible();
  });

  test('logout clears session and shows setup screen', async ({ page }) => {
    await completeSetup(page, {
      name: 'Logouter',
      postcode: '5000',
    });

    await expect(page.locator('.app-header')).toBeVisible();

    // Click logout
    await page.click('.logout-btn');

    // Should return to setup screen
    await expect(page.locator('.user-setup')).toBeVisible({ timeout: 5_000 });
    await expect(page.locator('.app-header')).not.toBeVisible();

    // Reload → should still be on setup (session cleared)
    await page.reload();
    await expect(
      page.locator('.user-setup, .app-header').first()
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('.user-setup')).toBeVisible();
  });
});

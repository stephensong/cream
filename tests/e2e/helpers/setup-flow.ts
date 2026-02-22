import { Page, expect } from '@playwright/test';
import { waitForAppLoad } from './wait-for-app';

export interface SetupOptions {
  name: string;
  postcode: string;
  password: string;
  isSupplier?: boolean;
  description?: string;
  locality?: string;
  /** Customer mode: supplier name to look up via rendezvous service. */
  supplierName?: string;
  /** Skip initial navigation (page already loaded via waitForAppLoadAt). */
  skipNav?: boolean;
}

/**
 * Complete the full setup wizard (profile + password) and land on the main app.
 */
export async function completeSetup(page: Page, opts: SetupOptions): Promise<void> {
  if (!opts.skipNav) {
    await waitForAppLoad(page);
  }

  // Step 1: Profile
  await expect(page.locator('.user-setup')).toBeVisible();

  await page.fill('input[placeholder="Name or moniker..."]', opts.name);
  await page.fill('input[placeholder="e.g. 2000"]', opts.postcode);

  // If a locality dropdown appears (multiple localities for this postcode), select one
  const localitySelect = page.locator('select');
  if (await localitySelect.isVisible({ timeout: 500 }).catch(() => false)) {
    const locality = opts.locality || '';
    if (locality) {
      await localitySelect.selectOption({ label: locality });
    } else {
      // Pick the first non-placeholder option
      await localitySelect.selectOption({ index: 1 });
    }
  }

  if (opts.isSupplier) {
    await page.check('input[type="checkbox"]');
    if (opts.description) {
      await page.fill('textarea[placeholder="Describe your farm or dairy..."]', opts.description);
    }
  }

  // Customer mode: look up supplier via rendezvous service
  if (opts.supplierName) {
    // Check if auto-connect already resolved (URL-based ?supplier= flow)
    const alreadyConnected = await page.locator('.welcome-back').isVisible({ timeout: 500 }).catch(() => false);
    if (!alreadyConnected) {
      // Manual lookup flow
      await page.fill('input[placeholder="e.g. garys-farm"]', opts.supplierName);
      await page.click('button:has-text("Look up")');
    }
    await expect(page.locator('.welcome-back')).toBeVisible({ timeout: 15_000 });
  }

  await page.click('button:has-text("Next")');

  // Step 2: Password
  await expect(page.locator('h1:has-text("Set a Password")')).toBeVisible();
  await page.fill('input[placeholder="Enter password..."]', opts.password);
  await page.fill('input[placeholder="Confirm password..."]', opts.password);
  await page.click('button:has-text("Get Started")');

  // Wait for main app to render
  await expect(page.locator('.app-header')).toBeVisible({ timeout: 15_000 });
}

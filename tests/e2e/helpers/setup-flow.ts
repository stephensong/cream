import { Page, expect } from '@playwright/test';
import { waitForAppLoad } from './wait-for-app';

export interface SetupOptions {
  name: string;
  postcode: string;
  password: string;
  isSupplier?: boolean;
  description?: string;
}

/**
 * Complete the full setup wizard (profile + password) and land on the main app.
 */
export async function completeSetup(page: Page, opts: SetupOptions): Promise<void> {
  await waitForAppLoad(page);

  // Step 1: Profile
  await expect(page.locator('.user-setup')).toBeVisible();

  await page.fill('input[placeholder="Name or moniker..."]', opts.name);
  await page.fill('input[placeholder="e.g. 2000"]', opts.postcode);

  if (opts.isSupplier) {
    await page.check('input[type="checkbox"]');
    if (opts.description) {
      await page.fill('textarea[placeholder="Describe your farm or dairy..."]', opts.description);
    }
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

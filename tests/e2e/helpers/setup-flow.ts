import { Page, expect } from '@playwright/test';
import { waitForAppLoad } from './wait-for-app';

export interface SetupOptions {
  name: string;
  postcode: string;
  isSupplier?: boolean;
  description?: string;
  locality?: string;
  /** Customer mode: supplier name to look up via rendezvous service. */
  supplierName?: string;
  /** Skip initial navigation (page already loaded via waitForAppLoadAt). */
  skipNav?: boolean;
}

/**
 * Complete the setup wizard and land on the main app.
 *
 * In dev mode the password is derived automatically from the name
 * (name.to_lowercase()), so there is no password screen.
 */
export async function completeSetup(page: Page, opts: SetupOptions): Promise<void> {
  if (!opts.skipNav) {
    await waitForAppLoad(page);
  }

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
    // Only check the box if it's not already checked (auto-fill may have set it)
    const checkbox = page.locator('input[type="checkbox"]');
    if (!(await checkbox.isChecked())) {
      await checkbox.check();
    }
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

  // Single-step: "Get Started" derives password and completes setup
  await page.click('button:has-text("Get Started")');

  // Wait for main app to render
  await expect(page.locator('.app-header')).toBeVisible({ timeout: 15_000 });
}

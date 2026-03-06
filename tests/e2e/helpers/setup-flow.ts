import { Page, expect } from '@playwright/test';
import { waitForAppLoad } from './wait-for-app';

export interface SetupOptions {
  name: string;
  postcode: string;
  isSupplier?: boolean;
  description?: string;
  locality?: string;
  /** Customer mode: supplier name to connect to (via dropdown or rendezvous). */
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

  // Wait for auto-fill to settle — if the name matches an existing supplier in
  // the directory, the oninput handler auto-fills postcode, locality, and checks
  // the supplier checkbox. We must let these re-renders complete before
  // interacting with other form elements.
  await page.waitForTimeout(1000);

  // Now fill postcode (overwriting any auto-filled value)
  await page.fill('input[placeholder="e.g. 2000"]', opts.postcode);
  // Wait for postcode oninput re-render (locality lookup)
  await page.waitForTimeout(500);

  // If a locality dropdown appears (multiple localities for this postcode), select one
  const localitySelect = page.locator('.form-group:has(label:text("Locality")) select');
  if (await localitySelect.isVisible({ timeout: 1000 }).catch(() => false)) {
    const locality = opts.locality || '';
    if (locality) {
      await localitySelect.selectOption({ label: locality });
    } else {
      // Pick the first non-placeholder option
      await localitySelect.selectOption({ index: 1 });
    }
  }

  // In auto-connect mode (?supplier= URL), the checkbox is not rendered
  const checkbox = page.locator('input[type="checkbox"]');
  const checkboxVisible = await checkbox.isVisible({ timeout: 500 }).catch(() => false);

  if (opts.isSupplier && checkboxVisible) {
    if (!(await checkbox.isChecked())) {
      await checkbox.check();
    }
    if (opts.description) {
      await page.fill('textarea[placeholder="Describe your farm or dairy..."]', opts.description);
    }
  } else if (!opts.isSupplier && checkboxVisible) {
    // Auto-fill may have checked the supplier box. Ensure it's unchecked.
    if (await checkbox.isChecked()) {
      await checkbox.uncheck();
    }
  }

  // Customer mode: connect to a supplier
  if (opts.supplierName && !opts.isSupplier) {
    // Check if auto-connect already resolved (URL-based ?supplier= flow)
    const alreadyConnected = await page.locator('.welcome-back').isVisible({ timeout: 500 }).catch(() => false);
    if (!alreadyConnected) {
      // Try the nearby suppliers dropdown first
      const supplierSelect = page.locator('.form-group:has(label:text("Connect to a supplier")) select');
      if (await supplierSelect.isVisible({ timeout: 5_000 }).catch(() => false)) {
        await supplierSelect.selectOption({ label: new RegExp(opts.supplierName) });
      }
    }
    // Wait for rendezvous lookup to complete
    await expect(page.locator('.welcome-back')).toBeVisible({ timeout: 15_000 });
  }

  // Single-step: "Get Started" derives password and completes setup
  await page.click('button:has-text("Get Started")');

  // Wait for main app to render
  await expect(page.locator('.app-header')).toBeVisible({ timeout: 15_000 });
}

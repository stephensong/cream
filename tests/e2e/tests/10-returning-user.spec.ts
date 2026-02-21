import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForAppLoad } from '../helpers/wait-for-app';

test.describe('Returning User Auto-fill', () => {
  test('logging out and re-entering name auto-fills from directory', async ({ page }) => {
    // Step 1: Register as a supplier
    await completeSetup(page, {
      name: 'ReturnUser',
      postcode: '2000',
      locality: 'Haymarket',
      password: 'returnpass',
      isSupplier: true,
      description: 'Returning user test farm',
    });

    // Verify we're logged in with correct info
    await expect(page.locator('.app-header')).toBeVisible();
    await expect(page.locator('.user-moniker')).toHaveText('Returnuser');
    await expect(page.locator('.user-postcode')).toContainText('Haymarket (2000)');
    await expect(page.locator('.supplier-badge')).toBeVisible();

    // Wait for directory to sync (so our entry is in SharedState)
    await page.waitForTimeout(3000);

    // Step 2: Log out
    await page.click('.logout-link');
    await expect(page.locator('.user-setup')).toBeVisible({ timeout: 5_000 });

    // Step 3: Type the name (lowercase) and tab out to trigger blur
    const nameInput = page.locator('input[placeholder="Name or moniker..."]');
    await nameInput.fill('returnuser');
    // Tab out to trigger onfocusout
    await nameInput.press('Tab');

    // Step 4: Verify auto-fill happened
    // Name should be corrected to directory casing
    await expect(nameInput).toHaveValue('Returnuser');

    // Welcome back message should appear
    await expect(page.locator('.welcome-back')).toContainText('Welcome back, Returnuser!');

    // Postcode should be filled
    const postcodeInput = page.locator('input[placeholder="e.g. 2000"]');
    await expect(postcodeInput).toHaveValue('2000');

    // Locality should be selected (either in dropdown or auto-selected)
    // For postcode 2000 which has multiple localities, a dropdown should appear
    // with Haymarket pre-selected
    const localitySelect = page.locator('select');
    if (await localitySelect.isVisible({ timeout: 500 }).catch(() => false)) {
      await expect(localitySelect).toHaveValue('Haymarket');
    }

    // Supplier checkbox should be checked
    await expect(page.locator('input[type="checkbox"]')).toBeChecked();

    // Description should be filled
    await expect(
      page.locator('textarea[placeholder="Describe your farm or dairy..."]')
    ).toHaveValue('Returning user test farm');

    // Step 5: Complete login with password and verify
    await page.click('button:has-text("Next")');
    await expect(page.locator('h1:has-text("Set a Password")')).toBeVisible();
    await page.fill('input[placeholder="Enter password..."]', 'returnpass');
    await page.fill('input[placeholder="Confirm password..."]', 'returnpass');
    await page.click('button:has-text("Get Started")');

    // Should be back in the app with correct info
    await expect(page.locator('.app-header')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.user-moniker')).toHaveText('Returnuser');
    await expect(page.locator('.user-postcode')).toContainText('Haymarket (2000)');
    await expect(page.locator('.supplier-badge')).toBeVisible();
  });
});

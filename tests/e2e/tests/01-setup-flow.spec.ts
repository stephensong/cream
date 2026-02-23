import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';

test.describe('Setup Flow', () => {
  test('register as a supplier and verify header', async ({ page }) => {
    await completeSetup(page, {
      name: 'TestSupplier',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy from happy cows',
    });

    // Verify header shows correct user info
    await expect(page.locator('.user-moniker')).toHaveText('Testsupplier');
    await expect(page.locator('.user-postcode')).toContainText('2000');
    await expect(page.locator('.supplier-badge')).toBeVisible();

    // Verify nav has supplier-specific button
    await expect(page.locator('button:has-text("My Storefront")')).toBeVisible();
    await expect(page.locator('button:has-text("Browse Suppliers")')).toBeVisible();
    await expect(page.locator('button:has-text("Wallet")')).toBeVisible();
  });

  test('register as a customer (no supplier badge)', async ({ page }) => {
    await completeSetup(page, {
      name: 'TestCustomer',
      postcode: '3000',
    });

    await expect(page.locator('.user-moniker')).toHaveText('Testcustomer');
    await expect(page.locator('.supplier-badge')).not.toBeVisible();
    await expect(page.locator('button:has-text("My Storefront")')).not.toBeVisible();
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Supplier Dashboard', () => {
  test('My Storefront shows harness products for Gary', async ({ page }) => {
    await completeSetup(page, {
      name: 'Gary',
      postcode: '2450',
      isSupplier: true,
      description: 'Real Beaut Dairy',
    });

    await waitForConnected(page);

    // Navigate to My Storefront
    await page.click('button:has-text("My Storefront")');
    await expect(page.locator('.supplier-dashboard')).toBeVisible();
    await expect(page.locator('h2:has-text("My Storefront")')).toBeVisible();

    // Wait for network storefront data to load and show products
    // Cumulative state: harness gives Gary 4 products, no prior tests add more.
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBe(4);
    }).toPass({ timeout: 15_000 });

    // Verify the "Your Products (N)" header reflects the count
    await expect(page.locator('h3', { hasText: /Your Products \(4\)/ })).toBeVisible();

    // Verify storefront info section
    await expect(page.locator('.dashboard-section', { hasText: 'Storefront Info' })).toContainText('Gary');
  });

  test('Add Product button toggles the form', async ({ page }) => {
    await completeSetup(page, {
      name: 'FormToggler',
      postcode: '3000',
      isSupplier: true,
      description: 'Testing form toggle',
    });

    await page.click('button:has-text("My Storefront")');
    await expect(page.locator('.supplier-dashboard')).toBeVisible();

    // Initially no add-product form visible
    await expect(page.locator('.add-product-form')).not.toBeVisible();

    // Click "Add Product" → form appears
    await page.click('button:has-text("Add Product")');
    await expect(page.locator('.add-product-form')).toBeVisible();

    // Click "Cancel" → form disappears
    await page.click('button:has-text("Cancel")');
    await expect(page.locator('.add-product-form')).not.toBeVisible();
  });
});

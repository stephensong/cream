import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Add Product', () => {
  test('add a product via the form and verify it appears', async ({ page }) => {
    await completeSetup(page, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });

    await waitForConnected(page);

    // Navigate to My Storefront
    await page.click('button:has-text("My Storefront")');
    await expect(page.locator('.supplier-dashboard')).toBeVisible();

    // Wait for existing products to load from network
    // Cumulative state: Gary has 4 products from harness, no prior tests add more.
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBe(4);
    }).toPass({ timeout: 15_000 });

    const initialCount = await page.locator('.product-card').count();

    // Open the add product form
    await page.click('button:has-text("Add Product")');
    await expect(page.locator('.add-product-form')).toBeVisible();

    // Fill in product details
    await page.fill('input[placeholder="e.g., Raw Whole Milk (1 gal)"]', 'Organic Goat Cheese');
    await page.selectOption('select', 'Cheese');
    await page.fill('input[placeholder="500"]', '950');
    await page.fill('input[placeholder="10"]', '8');
    await page.fill('textarea[placeholder="Describe your product..."]', 'Aged 6 months, tangy and delicious');

    // Submit
    await page.click('button:has-text("Save Product")');

    // Form should close after save
    await expect(page.locator('.add-product-form')).not.toBeVisible({ timeout: 5_000 });

    // Wait for the new product to appear in the product list (from network round-trip)
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThan(initialCount);
    }).toPass({ timeout: 20_000 });

    // Verify at least one product card with this name exists
    await expect(
      page.locator('.product-card', { hasText: 'Organic Goat Cheese' }).first()
    ).toBeVisible();
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('View Storefront', () => {
  test('navigate to a supplier storefront and see products', async ({ page }) => {
    await completeSetup(page, {
      name: 'Shopper',
      postcode: '2000',
      password: 'shopperpass',
    });

    await waitForConnected(page);
    await waitForSupplierCount(page, 3);

    // Click "View Storefront" on Gary's card
    const garyCard = page.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    // Verify we're on Gary's storefront view
    await expect(page.locator('.storefront-view')).toBeVisible();
    await expect(page.locator('.storefront-view h2')).toHaveText('Gary');

    // Should show products (Gary has 3 from harness)
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(3);
    }).toPass({ timeout: 15_000 });

    // Each product card should have an Order button (since this is not our storefront)
    const firstProduct = page.locator('.product-card').first();
    await expect(firstProduct.locator('button:has-text("Order")')).toBeVisible();

    // Verify product details are shown
    await expect(firstProduct.locator('.category')).toBeVisible();
    await expect(firstProduct.locator('.price')).toBeVisible();
    await expect(firstProduct.locator('.quantity')).toBeVisible();
  });

  test('own storefront shows note and hides Order buttons', async ({ page }) => {
    await completeSetup(page, {
      name: 'Gary',
      postcode: '2000',
      password: 'gary',
      isSupplier: true,
      description: 'Fresh dairy products',
    });

    await waitForConnected(page);

    // Navigate to own storefront via directory
    const garyCard = page.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    await expect(page.locator('.storefront-view')).toBeVisible();
    await expect(page.locator('.own-storefront-note')).toBeVisible();

    // Wait for products to load
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Order buttons should NOT be visible on own storefront
    await expect(page.locator('.product-card button:has-text("Order")')).not.toBeVisible();
  });
});

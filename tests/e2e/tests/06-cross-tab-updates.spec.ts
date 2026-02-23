import { test, expect, Browser } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Cross-Tab Updates', () => {
  test('Gary adds a product, Emma sees the updated count in directory', async ({ browser }) => {
    // Create two independent browser contexts (separate sessionStorage = separate identities)
    const garyContext = await browser.newContext();
    const emmaContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const emmaPage = await emmaContext.newPage();

    // Gary registers as a supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Emma registers as a customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
      isSupplier: true,
      description: 'Organic dairy',
    });
    await waitForConnected(emmaPage);

    // Wait for Emma to see the initial directory with suppliers
    await waitForSupplierCount(emmaPage, 3);

    // Read Gary's current product count (grows cumulatively across runs)
    const garyCardOnEmma = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    const countText = await garyCardOnEmma.locator('.product-count').textContent({ timeout: 15_000 });
    const initialCount = parseInt(countText!);
    expect(initialCount).toBeGreaterThanOrEqual(5); // 4 harness + 1 from test-04

    // Gary navigates to My Storefront and adds a product
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    await garyPage.click('button:has-text("Add Product")');
    await expect(garyPage.locator('.add-product-form')).toBeVisible();

    await garyPage.fill('input[placeholder="e.g., Raw Whole Milk (1 gal)"]', 'Cross-Tab Test Milk');
    await garyPage.fill('input[placeholder="500"]', '750');
    await garyPage.fill('input[placeholder="10"]', '5');
    await garyPage.fill('textarea[placeholder="Describe your product..."]', 'Product added for cross-tab test');
    await garyPage.click('button:has-text("Save Product")');

    // Wait for the form to close (product saved)
    await expect(garyPage.locator('.add-product-form')).not.toBeVisible({ timeout: 5_000 });

    // Now verify that Emma's directory view updates with the new product count.
    // This tests the full pipeline: Gary's WASM → WebSocket → Freenet node →
    // subscription notification → Emma's WebSocket → WASM re-render.
    const expectedCount = `${initialCount + 1} products`;
    await expect(garyCardOnEmma.locator('.product-count')).toHaveText(expectedCount, { timeout: 30_000 });

    // Cleanup
    await garyContext.close();
    await emmaContext.close();
  });
});

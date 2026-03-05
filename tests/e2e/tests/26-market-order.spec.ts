import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Market Order Flow', () => {
  test('customer browses market and navigates to supplier to order', async ({ browser }) => {
    const garyContext = await browser.newContext();
    const aliceContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const alicePage = await aliceContext.newPage();

    // Gary registers as supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2450',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Alice registers as customer
    await completeSetup(alicePage, {
      name: 'Alice',
      postcode: '2000',
    });
    await waitForConnected(alicePage);
    await waitForSupplierCount(alicePage, 3);

    // Alice clicks the harness market
    const marketCard = alicePage.locator('.market-card', { hasText: 'Coffs Harbour Farmers Market' });
    await expect(marketCard).toBeVisible({ timeout: 30_000 });
    await marketCard.locator('a:has-text("View Market")').click();

    // Verify market view
    await expect(alicePage.locator('.market-view h2')).toHaveText('Coffs Harbour Farmers Market');

    // Verify products are listed
    await expect(async () => {
      const count = await alicePage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Find an in-stock product from Gary and click the order link
    const garyProduct = alicePage.locator('.product-card', { hasText: 'Gary' }).first();
    await expect(garyProduct).toBeVisible();
    await garyProduct.locator('a:has-text("Order from Gary")').click();

    // Alice should be on Gary's storefront now
    await expect(alicePage.locator('.storefront-view')).toBeVisible({ timeout: 10_000 });
    await expect(alicePage.locator('.storefront-view h2')).toHaveText('Gary');

    // Cleanup
    await garyContext.close();
    await aliceContext.close();
  });
});

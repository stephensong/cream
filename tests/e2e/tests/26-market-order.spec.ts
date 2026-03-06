import { test, expect } from '../helpers/with-invariants';
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

    // Alice navigates to Markets and clicks the harness market
    await alicePage.click('button:has-text("Markets")');
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

    // Find any in-stock product with an order link and click it
    // (stock depletes across cumulative E2E runs, so don't target a specific supplier)
    const orderableProduct = alicePage.locator('.product-card:has(.order-link)').first();
    await expect(orderableProduct).toBeVisible({ timeout: 15_000 });
    const orderLink = orderableProduct.locator('.order-link');
    const linkText = await orderLink.textContent();
    const supplierName = linkText!.replace('Order from ', '');
    await orderLink.click();

    // Alice should be on the supplier's storefront now
    await expect(alicePage.locator('.storefront-view')).toBeVisible({ timeout: 10_000 });
    await expect(alicePage.locator('.storefront-view h2')).toHaveText(supplierName);

    // Cleanup
    await garyContext.close();
    await aliceContext.close();
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Place Order', () => {
  test('Emma places an order and Gary sees it in incoming orders', async ({ browser }) => {
    // Create two independent browser contexts (separate identities)
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
    });
    await waitForConnected(emmaPage);

    // Wait for Emma to see the directory with suppliers
    await waitForSupplierCount(emmaPage, 3);

    // Emma navigates to Gary's storefront
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    // Verify Emma is on Gary's storefront
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();
    await expect(emmaPage.locator('.storefront-view h2')).toHaveText('Gary');

    // Wait for products to load
    await expect(async () => {
      const count = await emmaPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Emma clicks "Order" on the first product
    const firstProduct = emmaPage.locator('.product-card').first();
    await firstProduct.locator('button:has-text("Order")').click();

    // Verify the order form is shown
    await expect(emmaPage.locator('.order-form')).toBeVisible();

    // Set quantity to 2
    await emmaPage.fill('.order-form input[type="number"]', '2');

    // Select deposit tier (use the default: 2-Day Reserve)
    // Submit the order
    await emmaPage.click('.order-form button:has-text("Place Order")');

    // Verify order confirmation is shown
    await expect(emmaPage.locator('.order-confirmation')).toBeVisible();
    await expect(emmaPage.locator('.order-confirmation h3')).toHaveText('Order Submitted!');

    // Now check Gary's side — navigate to My Storefront
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    // Wait for Gary to see the incoming order via network subscription.
    // Cumulative state: 3 expired orders from harness + 1 new Reserved = 4 total.
    await expect(
      garyPage.locator('.dashboard-section h3', { hasText: 'Incoming Orders (4)' })
    ).toBeVisible({ timeout: 30_000 });

    // Verify a Reserved order card is visible (harness leaves expired orders too)
    const orderCard = garyPage.locator('.order-card', { hasText: 'Reserved' }).first();
    await expect(orderCard).toBeVisible();
    await expect(orderCard.locator('.order-id')).toContainText('Order #');

    // Cleanup
    await garyContext.close();
    await emmaContext.close();
  });
});

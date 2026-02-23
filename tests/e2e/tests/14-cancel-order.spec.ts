import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

/** Extract the numeric CURD balance from a wallet button's text. */
async function getWalletBalance(page: import('@playwright/test').Page): Promise<number> {
  const text = await page.locator('button:has-text("Wallet")').textContent();
  const match = text?.match(/(\d+)\s*CURD/);
  return match ? parseInt(match[1], 10) : 0;
}

test.describe('Cancel Order', () => {
  test('Gary cancels a Reserved order and deposit is no longer counted', async ({ browser }) => {
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

    // Emma registers as a customer and places an order to ensure a Reserved order exists
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);
    await waitForSupplierCount(emmaPage, 3);

    // Emma navigates to Gary's storefront and places an order
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    // Find a product with stock and order 1 unit
    const product = emmaPage.locator('.product-card').first();
    await product.locator('button:has-text("Order")').click();
    await expect(emmaPage.locator('.order-form')).toBeVisible();
    await emmaPage.fill('.order-form input[type="number"]', '1');
    await emmaPage.click('.order-form button:has-text("Place Order")');
    await expect(emmaPage.locator('.order-confirmation')).toBeVisible();

    // Navigate Gary to My Storefront and wait for the Reserved order
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    await expect(async () => {
      const cancelBtns = await garyPage.locator('.cancel-order-btn').count();
      expect(cancelBtns).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 30_000 });

    // Record initial wallet balance (includes deposits from active orders)
    const initialBalance = await getWalletBalance(garyPage);

    // Find a Reserved order card and click Cancel
    const reservedOrder = garyPage.locator('.order-card', { hasText: 'Reserved' }).first();
    await expect(reservedOrder).toBeVisible();
    const cancelBtn = reservedOrder.locator('.cancel-order-btn');
    await cancelBtn.click();

    // Assert the order now shows Cancelled status
    await expect(async () => {
      const cancelledOrders = await garyPage.locator('.order-card', { hasText: 'Cancelled' }).count();
      expect(cancelledOrders).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Assert wallet balance decreased (cancelled order deposit no longer counted)
    await expect(async () => {
      const currentBalance = await getWalletBalance(garyPage);
      expect(currentBalance).toBeLessThan(initialBalance);
    }).toPass({ timeout: 15_000 });

    // Assert the Cancel button is gone from the cancelled order
    const cancelledOrder = garyPage.locator('.order-card', { hasText: 'Cancelled' }).first();
    await expect(cancelledOrder.locator('.cancel-order-btn')).toHaveCount(0);

    await garyContext.close();
    await emmaContext.close();
  });
});

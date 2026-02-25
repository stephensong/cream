import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

/** Extract the numeric CURD balance from a wallet button's text. */
async function getWalletBalance(page: import('@playwright/test').Page): Promise<number> {
  const text = await page.locator('button:has-text("Wallet")').textContent();
  const match = text?.match(/(\d+)\s*CURD/);
  return match ? parseInt(match[1], 10) : 0;
}

test.describe('Fulfill Order', () => {
  test('Gary fulfills a Reserved order and deposit is settled to his wallet', async ({ browser }) => {
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

    // Emma registers as a customer and places an order
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
      const fulfillBtns = await garyPage.locator('.fulfill-order-btn').count();
      expect(fulfillBtns).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 30_000 });

    // Record initial wallet balance
    const initialBalance = await getWalletBalance(garyPage);

    // Find a Reserved order card and click Mark Fulfilled
    const reservedOrder = garyPage.locator('.order-card', { hasText: 'Reserved' }).first();
    await expect(reservedOrder).toBeVisible();
    const fulfillBtn = reservedOrder.locator('.fulfill-order-btn');
    await fulfillBtn.click();

    // Assert the order now shows Fulfilled status
    await expect(async () => {
      const fulfilledOrders = await garyPage.locator('.order-card', { hasText: 'Fulfilled' }).count();
      expect(fulfilledOrders).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Assert wallet balance increased (settled escrow deposit)
    await expect(async () => {
      const currentBalance = await getWalletBalance(garyPage);
      expect(currentBalance).toBeGreaterThan(initialBalance);
    }).toPass({ timeout: 15_000 });

    // Assert the Fulfill and Cancel buttons are gone from the fulfilled order
    const fulfilledOrder = garyPage.locator('.order-card', { hasText: 'Fulfilled' }).first();
    await expect(fulfilledOrder.locator('.fulfill-order-btn')).toHaveCount(0);
    await expect(fulfilledOrder.locator('.cancel-order-btn')).toHaveCount(0);

    await garyContext.close();
    await emmaContext.close();
  });
});

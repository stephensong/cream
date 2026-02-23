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
    const garyPage = await garyContext.newPage();

    // Gary registers as a supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Navigate to My Storefront and wait for orders to load
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    // Wait for storefront data (cumulative: orders from previous tests)
    await waitForSupplierCount(garyPage, 3);
    await garyPage.waitForTimeout(2000);

    // Wait until at least one Reserved order is visible with a Cancel button
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
  });
});

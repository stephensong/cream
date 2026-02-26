import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

/** Extract the numeric CURD balance from a wallet button's text. */
async function getWalletBalance(page: import('@playwright/test').Page): Promise<number> {
  const text = await page.locator('button:has-text("Wallet")').textContent();
  const match = text?.match(/(\d+)\s*CURD/);
  return match ? parseInt(match[1], 10) : 0;
}


test.describe('Wallet Transfer Ledger', () => {
  test('Order deposit and fulfillment produce correct ledger entries', async ({ browser }) => {
    const garyContext = await browser.newContext();
    const emmaContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const emmaPage = await emmaContext.newPage();

    // Register Gary as supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Register Emma as customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);

    // Wait for Emma's balance to be available
    await expect(async () => {
      const balance = await getWalletBalance(emmaPage);
      expect(balance).toBeGreaterThanOrEqual(10_000);
    }).toPass({ timeout: 15_000 });

    // Emma navigates to Gary's storefront and places an order
    await waitForSupplierCount(emmaPage, 3);
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    // Wait for products to load
    await expect(async () => {
      const count = await emmaPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Read price and place order (1 unit, default 2-Day Reserve 10%)
    const priceText = await emmaPage.locator('.product-card').first().locator('.price').textContent();
    const pricePerUnit = parseInt(priceText!.replace(/[^0-9]/g, ''), 10);
    const expectedDeposit = Math.floor(pricePerUnit / 10);

    await emmaPage.locator('.product-card').first().locator('button:has-text("Order")').click();
    await expect(emmaPage.locator('.order-form')).toBeVisible();
    await emmaPage.fill('.order-form input[type="number"]', '1');
    await emmaPage.click('.order-form button:has-text("Place Order")');
    await expect(emmaPage.locator('.order-confirmation')).toBeVisible();

    // Emma: navigate to wallet and verify debit ledger entry
    await emmaPage.click('button:has-text("Wallet")');
    await expect(emmaPage.locator('.wallet-balance')).toBeVisible();

    // Assert debit row for order deposit (skip balance check — not idempotent across runs)
    const depositRow = emmaPage.locator('.tx-history tbody tr', { hasText: 'Order deposit' });
    await expect(async () => {
      const count = await depositRow.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 10_000 });
    await expect(depositRow.first().locator('td.tx-debit')).toContainText(`-${expectedDeposit}`);

    // Gary: navigate to My Storefront and fulfill the order
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    await expect(async () => {
      const count = await garyPage.locator('.fulfill-order-btn').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 30_000 });

    // Find a Reserved order and fulfill it
    const reservedOrder = garyPage.locator('.order-card', { hasText: 'Reserved' }).first();
    await expect(reservedOrder).toBeVisible();
    await reservedOrder.locator('.fulfill-order-btn').click();

    // Wait for Fulfilled status to appear
    await expect(async () => {
      const fulfilledOrders = await garyPage.locator('.order-card', { hasText: 'Fulfilled' }).count();
      expect(fulfilledOrders).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Gary: navigate to wallet and verify credit ledger entry for escrow settlement
    await garyPage.click('button:has-text("Wallet")');
    await expect(garyPage.locator('.wallet-balance')).toBeVisible();

    const settlementRow = garyPage.locator('.tx-history tbody tr', { hasText: 'Escrow settlement' });
    await expect(async () => {
      const count = await settlementRow.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });
    await expect(settlementRow.first().locator('td.tx-credit')).toContainText('+');

    await garyContext.close();
    await emmaContext.close();
  });
});

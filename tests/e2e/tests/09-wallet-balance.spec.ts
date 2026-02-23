import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

/** Extract the numeric CURD balance from a wallet button's text. */
async function getWalletBalance(page: import('@playwright/test').Page): Promise<number> {
  const text = await page.locator('button:has-text("Wallet")').textContent();
  const match = text?.match(/(\d+)\s*CURD/);
  return match ? parseInt(match[1], 10) : 0;
}

test.describe('Wallet Balance', () => {
  test('Emma deposit deducted on order, Gary receives deposit credit', async ({ browser }) => {
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

    // Record initial balances.
    // Wallet balance is local UI state (not network state), so each fresh
    // browser context starts at the default 10,000 CURD.
    const emmaInitial = await getWalletBalance(emmaPage);
    expect(emmaInitial).toBe(10_000);

    // Wait for Gary's balance to stabilize (subscriptions may still be arriving)
    await waitForSupplierCount(garyPage, 3);
    // Small delay to let any pending storefront subscriptions resolve
    await garyPage.waitForTimeout(2000);
    const garyInitial = await getWalletBalance(garyPage);

    // Emma navigates to Gary's storefront
    await waitForSupplierCount(emmaPage, 3);
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    // Cumulative state: Gary has 6 products (4 harness + test-04 + test-06)
    await expect(async () => {
      const count = await emmaPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(6);
    }).toPass({ timeout: 15_000 });

    // Read the first product's price
    const priceText = await emmaPage.locator('.product-card').first().locator('.price').textContent();
    const pricePerUnit = parseInt(priceText!.replace(/[^0-9]/g, ''), 10);

    // Emma orders 2 units with 2-Day Reserve (10% deposit)
    await emmaPage.locator('.product-card').first().locator('button:has-text("Order")').click();
    await expect(emmaPage.locator('.order-form')).toBeVisible();
    await emmaPage.fill('.order-form input[type="number"]', '2');
    // Default tier is "2-Day Reserve (10%)" — keep it
    await emmaPage.click('.order-form button:has-text("Place Order")');

    // Verify order confirmation
    await expect(emmaPage.locator('.order-confirmation')).toBeVisible();

    // Calculate expected deposit: 10% of (price * 2)
    const totalPrice = pricePerUnit * 2;
    const expectedDeposit = Math.floor(totalPrice / 10);

    // Emma's wallet should show the deducted balance
    const expectedEmmaBalance = emmaInitial - expectedDeposit;
    await expect(emmaPage.locator('button:has-text("Wallet")')).toContainText(`${expectedEmmaBalance} CURD`);

    // Navigate to Emma's wallet to verify the detailed view
    await emmaPage.click('button:has-text("Wallet")');
    await expect(emmaPage.locator('.wallet-balance')).toContainText(`${expectedEmmaBalance} CURD`);

    // Wait for Gary's wallet button to increase by the deposit amount
    const expectedGaryBalance = garyInitial + expectedDeposit;
    await expect(async () => {
      const currentBalance = await getWalletBalance(garyPage);
      expect(currentBalance).toBe(expectedGaryBalance);
    }).toPass({ timeout: 30_000 });

    // Navigate to Gary's wallet to verify the detailed view
    await garyPage.click('button:has-text("Wallet")');
    await expect(garyPage.locator('.wallet-balance')).toContainText(`${expectedGaryBalance} CURD`);
    await expect(garyPage.locator('.wallet-deposits')).toBeVisible();

    await garyContext.close();
    await emmaContext.close();
  });
});

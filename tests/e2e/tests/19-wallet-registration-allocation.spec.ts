import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

/** Extract the numeric CURD balance from a wallet button's text. */
async function getWalletBalance(page: import('@playwright/test').Page): Promise<number> {
  const text = await page.locator('button:has-text("Wallet")').textContent();
  const match = text?.match(/(\d+)\s*CURD/);
  return match ? parseInt(match[1], 10) : 0;
}

/** Wait for the wallet balance to stabilize (stop changing). */
async function waitForBalanceStable(page: import('@playwright/test').Page, timeout = 10_000): Promise<number> {
  let prev = -1;
  let current = await getWalletBalance(page);
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    await page.waitForTimeout(1000);
    prev = current;
    current = await getWalletBalance(page);
    if (current === prev && current > 0) {
      return current;
    }
  }
  return current;
}

test.describe('Wallet Registration Allocation', () => {
  test('User has initial 10,000 CURD allocation with ledger entry', async ({ browser }) => {
    const emmaContext = await browser.newContext();
    const emmaPage = await emmaContext.newPage();

    // Register Emma (harness-preregistered customer)
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);

    // Wait for balance to stabilize
    const balance = await waitForBalanceStable(emmaPage);
    expect(balance).toBeGreaterThanOrEqual(10_000);

    // Navigate to wallet view
    await emmaPage.click('button:has-text("Wallet")');
    await expect(emmaPage.locator('.wallet-balance')).toBeVisible();

    // Assert tx-history has an initial allocation credit entry
    const allocationRow = emmaPage.locator('.tx-history tbody tr', { hasText: 'Initial CURD allocation' });
    await expect(async () => {
      const count = await allocationRow.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 10_000 });
    await expect(allocationRow.first().locator('td.tx-credit')).toContainText('+10000');

    // Assert counterparty shows "System" (the root user)
    const cells = allocationRow.first().locator('td');
    // Columns: Time | Description | Counterparty | Amount
    await expect(cells.nth(2)).toContainText('System');

    await emmaContext.close();
  });
});

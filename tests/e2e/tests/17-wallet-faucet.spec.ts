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

test.describe('Wallet Faucet', () => {
  test('Faucet top-up adds 1000 CURD with ledger entry', async ({ browser }) => {
    const emmaContext = await browser.newContext();
    const emmaPage = await emmaContext.newPage();

    // Register Emma as a customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);

    // Wait for balance to stabilize after registration
    const initialBalance = await waitForBalanceStable(emmaPage);
    expect(initialBalance).toBeGreaterThanOrEqual(10_000);

    // Navigate to wallet view
    await emmaPage.click('button:has-text("Wallet")');
    await expect(emmaPage.locator('.wallet-balance')).toBeVisible();

    // Click faucet button
    await emmaPage.click('button:has-text("Faucet (+1000 CURD)")');

    // Assert balance increased by 1000
    await expect(async () => {
      const balance = await getWalletBalance(emmaPage);
      expect(balance).toBeGreaterThanOrEqual(initialBalance + 1000);
    }).toPass({ timeout: 15_000 });

    // Assert wallet-balance detail view also reflects the increase
    await expect(async () => {
      const text = await emmaPage.locator('.wallet-balance').textContent();
      const match = text?.match(/(\d+)/);
      const balance = match ? parseInt(match[1], 10) : 0;
      expect(balance).toBeGreaterThanOrEqual(initialBalance + 1000);
    }).toPass({ timeout: 15_000 });

    // Assert tx-history has at least one faucet credit entry
    const faucetRows = emmaPage.locator('.tx-history tbody tr', { hasText: 'Faucet' });
    await expect(async () => {
      const count = await faucetRows.count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 10_000 });
    await expect(faucetRows.first().locator('td.tx-credit')).toContainText('+1000');

    await emmaContext.close();
  });
});

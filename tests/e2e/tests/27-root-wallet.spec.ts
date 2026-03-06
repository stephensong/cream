import { test, expect } from '../helpers/with-invariants';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Root Wallet', () => {
  test('root login shows system root balance (not 10K user allocation)', async ({ browser }) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();

    // Log in as "root" — should use the system root identity
    await completeSetup(page, {
      name: 'root',
      postcode: '2000',
    });
    await waitForConnected(page);

    // The nav bar should show "[Root]" as the role badge
    await expect(page.locator('.role-badge')).toContainText('Root', { timeout: 10_000 });

    // The wallet balance in the nav should be much greater than 10,000
    // (system root starts with 1,000,000 and allocates 10K per user)
    await expect(async () => {
      const text = await page.locator('button:has-text("Wallet")').textContent();
      const match = text?.match(/([\d,]+)\s*CURD/);
      const balance = match ? parseInt(match[1].replace(/,/g, ''), 10) : 0;
      expect(balance).toBeGreaterThan(100_000);
    }).toPass({ timeout: 30_000 });

    // Navigate to wallet view and verify the detailed balance
    await page.click('button:has-text("Wallet")');
    await expect(async () => {
      const text = await page.locator('.wallet-balance').textContent();
      const match = text?.match(/([\d,]+)\s*CURD/);
      const balance = match ? parseInt(match[1].replace(/,/g, ''), 10) : 0;
      expect(balance).toBeGreaterThan(100_000);
    }).toPass({ timeout: 15_000 });

    // The ledger should show genesis and allocation transactions
    await expect(page.locator('.tx-history tbody tr').first()).toBeVisible({ timeout: 10_000 });
    const txCount = await page.locator('.tx-history tbody tr').count();
    expect(txCount).toBeGreaterThanOrEqual(1);

    await ctx.close();
  });
});

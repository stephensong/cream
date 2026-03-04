import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { dismissDxToast } from '../helpers/wait-for-app';

test.describe('Toll Rates', () => {
  test('root saves toll rates, other user sees updated rates in wallet', async ({ browser }) => {
    const rootCtx = await browser.newContext();
    const userCtx = await browser.newContext();
    const rootPage = await rootCtx.newPage();
    const userPage = await userCtx.newPage();

    // ── Step 1: Log in as "root" (pre-seeded admin) ──
    await completeSetup(rootPage, {
      name: 'root',
      postcode: '2000',
    });

    // Root should see the "Root" nav button
    try {
      await expect(rootPage.locator('button:has-text("Root")')).toBeVisible({ timeout: 10_000 });
    } catch {
      await rootPage.reload();
      await expect(rootPage.locator('.app-header')).toBeVisible({ timeout: 15_000 });
      await dismissDxToast(rootPage);
      await expect(rootPage.locator('button:has-text("Root")')).toBeVisible({ timeout: 15_000 });
    }

    // ── Step 2: Log in as Gary (existing supplier, sees toll rates in wallet) ──
    await completeSetup(userPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Test dairy',
    });

    // Gary navigates to Wallet and reads current toll rates
    await userPage.click('button:has-text("Wallet")');
    await expect(userPage.locator('.fee-schedule')).toBeVisible({ timeout: 10_000 });

    // ── Step 3: Root navigates to Guardian Admin and changes toll rates ──
    await rootPage.click('button:has-text("Root")');
    await expect(rootPage.locator('.guardian-admin')).toBeVisible({ timeout: 10_000 });
    await expect(rootPage.locator('h3:has-text("Toll Rates")')).toBeVisible({ timeout: 10_000 });

    // Change CURD per Sat from default (10) to 25
    await rootPage.locator('.form-grid input').nth(3).fill('25');
    // Change Session Toll from default (1) to 5
    await rootPage.locator('.form-grid input').nth(0).fill('5');
    // Change Inbox Message from default (1) to 3
    await rootPage.locator('.form-grid input').nth(2).fill('3');

    // Click Save
    await rootPage.click('button:has-text("Save Toll Rates")');

    // Verify success feedback
    await expect(rootPage.locator('.alert-success:has-text("Toll rates saved")')).toBeVisible({ timeout: 10_000 });

    // Wait for the Freenet UpdateResponse to propagate
    await rootPage.waitForTimeout(8000);

    // ── Step 4: Gary refreshes and sees updated toll rates ──
    await userPage.reload();
    await expect(userPage.locator('.app-header')).toBeVisible({ timeout: 15_000 });
    await dismissDxToast(userPage);

    await userPage.click('button:has-text("Wallet")');
    await expect(userPage.locator('.fee-schedule')).toBeVisible({ timeout: 10_000 });

    // Verify the updated values appear
    await expect(userPage.locator('.toll-rates-compact p:has-text("1 sat = 25 CURD")')).toBeVisible({ timeout: 15_000 });
    await expect(userPage.locator('.toll-rates-compact p:has-text("Chat Session: 5 CURD")')).toBeVisible({ timeout: 15_000 });
    await expect(userPage.locator('.toll-rates-compact p:has-text("Send Message: 3 CURD")')).toBeVisible({ timeout: 15_000 });

    // ── Cleanup: restore default toll rates ──
    await rootPage.click('button:has-text("Root")');
    await expect(rootPage.locator('h3:has-text("Toll Rates")')).toBeVisible({ timeout: 10_000 });

    await rootPage.locator('.form-grid input').nth(0).fill('1');   // session_toll
    await rootPage.locator('.form-grid input').nth(1).fill('10');  // session_interval
    await rootPage.locator('.form-grid input').nth(2).fill('1');   // inbox_message
    await rootPage.locator('.form-grid input').nth(3).fill('10');  // curd_per_sat

    await rootPage.click('button:has-text("Save Toll Rates")');
    await expect(rootPage.locator('.alert-success:has-text("Toll rates saved")')).toBeVisible({ timeout: 10_000 });

    await rootCtx.close();
    await userCtx.close();
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForAppLoadAt, waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';
// Note: waitForConnected only works on pages with directory_view (supplier mode)

const APP_URL = 'http://localhost:8080';

test.describe('Customer Rendezvous Flow', () => {
  test('supplier registers, customer looks up via rendezvous and sees storefront', async ({ browser }) => {
    const supplierContext = await browser.newContext();
    const customerContext = await browser.newContext();

    const supplierPage = await supplierContext.newPage();
    const customerPage = await customerContext.newPage();

    // ── Supplier side: register as Gary ──────────────────────────────────────
    await waitForAppLoadAt(supplierPage, APP_URL);
    await completeSetup(supplierPage, {
      name: 'Gary',
      postcode: '2000',
      password: 'gary',
      isSupplier: true,
      description: 'Fresh dairy products',
      skipNav: true,
    });
    await waitForConnected(supplierPage);

    // Wait for directory to populate (integration test harness data)
    await waitForSupplierCount(supplierPage, 2);

    // Give the fire-and-forget rendezvous registration time to complete
    await supplierPage.waitForTimeout(3_000);

    // ── Customer side: auto-connect via ?supplier= URL param ─────────────────
    await waitForAppLoadAt(customerPage, `${APP_URL}/?supplier=gary`);
    await completeSetup(customerPage, {
      name: 'Alice',
      postcode: '3000',
      password: 'alice123',
      supplierName: 'gary',
      skipNav: true,
    });
    // Customer mode has no directory view, so no .connection-status element.
    // The app header being visible (from completeSetup) is sufficient proof of connection.

    // ── Assertions: customer UI shows single-storefront nav ──────────────────
    await expect(customerPage.locator('button:has-text("Storefront")')).toBeVisible();
    await expect(customerPage.locator('button:has-text("Browse Suppliers")')).not.toBeVisible();
    await expect(customerPage.locator('button:has-text("My Storefront")')).not.toBeVisible();

    // Navigate to the storefront
    await customerPage.click('button:has-text("Storefront")');
    await expect(customerPage.locator('.storefront-view')).toBeVisible({ timeout: 15_000 });

    // Should see Gary's products from the integration test harness
    await expect(async () => {
      const count = await customerPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(3);
    }).toPass({ timeout: 20_000 });

    // Products should have Order buttons
    const firstProduct = customerPage.locator('.product-card').first();
    await expect(firstProduct.locator('button:has-text("Order")')).toBeVisible();

    // Cleanup
    await supplierContext.close();
    await customerContext.close();
  });

  test('customer sees error for unknown supplier name', async ({ page }) => {
    await waitForAppLoadAt(page, APP_URL);

    // Fill basic profile fields
    await expect(page.locator('.user-setup')).toBeVisible();
    await page.fill('input[placeholder="Name or moniker..."]', 'Bob');
    await page.fill('input[placeholder="e.g. 2000"]', '4000');

    // Look up a non-existent supplier
    await page.fill('input[placeholder="e.g. garys-farm"]', 'nonexistent-farm');
    await page.click('button:has-text("Look up")');

    // Should show an error
    await expect(page.locator('.field-error')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.field-error')).toContainText('not found', { ignoreCase: true });

    // Next button should be disabled (no valid supplier lookup)
    await expect(page.locator('button:has-text("Next")')).toBeDisabled();
  });
});

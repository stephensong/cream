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

    // Cumulative state: Gary has 6 products (4 harness + test-04 + test-06).
    // Rendezvous tests run in a separate project so they may run on a fresh
    // or cumulative network — use >= to stay resilient.
    await expect(async () => {
      const count = await customerPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(4);
    }).toPass({ timeout: 20_000 });

    // Products should have Order buttons
    const firstProduct = customerPage.locator('.product-card').first();
    await expect(firstProduct.locator('button:has-text("Order")')).toBeVisible();

    // Cleanup
    await supplierContext.close();
    await customerContext.close();
  });

  // Note: "unknown supplier name" test removed — the setup screen now uses a
  // dropdown populated from the directory, so users cannot enter arbitrary names.
});

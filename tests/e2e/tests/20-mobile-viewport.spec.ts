import { test, expect, Page } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

/**
 * Mobile viewport layout tests.
 *
 * These verify that all key views render without layout breakage at common
 * mobile screen sizes. They check for:
 *   - No horizontal overflow (scrollbar)
 *   - Key elements visible and not clipped
 *   - Interactive elements (buttons, inputs) reachable
 *
 * What these do NOT test (requires manual inspection):
 *   - Aesthetic quality, spacing, visual balance
 *   - Touch target sizing (44px minimum)
 *   - Font readability on physical screens
 */

const MOBILE_VIEWPORTS = [
  { name: 'iPhone SE', width: 375, height: 667 },
  { name: 'Pixel 7', width: 412, height: 915 },
];

/** Assert no horizontal overflow on the page. */
async function assertNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    return document.documentElement.scrollWidth > document.documentElement.clientWidth;
  });
  expect(overflow, 'Page has horizontal overflow (scrollbar)').toBe(false);
}

/** Assert an element is within the viewport horizontally. */
async function assertWithinViewport(page: Page, selector: string) {
  const box = await page.locator(selector).first().boundingBox();
  if (box) {
    const viewport = page.viewportSize()!;
    expect(box.x + box.width, `${selector} overflows right edge`).toBeLessThanOrEqual(viewport.width + 1);
    expect(box.x, `${selector} overflows left edge`).toBeGreaterThanOrEqual(-1);
  }
}

for (const vp of MOBILE_VIEWPORTS) {
  test.describe(`Mobile layout: ${vp.name} (${vp.width}x${vp.height})`, () => {

    test('setup screen renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      // Load the setup screen
      await page.goto('http://localhost:8080');
      await expect(page.locator('.user-setup')).toBeVisible({ timeout: 30_000 });

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.user-setup');

      // All form elements visible and reachable
      await expect(page.locator('input[placeholder="Name or moniker..."]')).toBeVisible();
      await expect(page.locator('input[placeholder="e.g. 2000"]')).toBeVisible();
      await expect(page.locator('button:has-text("Get Started")')).toBeVisible();

      await context.close();
    });

    test('supplier dashboard renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      await completeSetup(page, {
        name: 'Gary',
        postcode: '2450',
        isSupplier: true,
        description: 'Real Beaut Dairy',
      });
      await waitForConnected(page);

      // Navigate to My Storefront
      await page.click('button:has-text("My Storefront")');
      await expect(page.locator('.supplier-dashboard')).toBeVisible();

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.supplier-dashboard');

      // Nav buttons visible
      await expect(page.locator('button:has-text("Browse Suppliers")')).toBeVisible();
      await expect(page.locator('button:has-text("Wallet")')).toBeVisible();

      // Product cards don't overflow
      await expect(async () => {
        const count = await page.locator('.product-card').count();
        expect(count).toBeGreaterThanOrEqual(1);
      }).toPass({ timeout: 15_000 });
      await assertWithinViewport(page, '.product-card');

      await context.close();
    });

    test('directory view renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      await completeSetup(page, {
        name: 'Emma',
        postcode: '2500',
      });
      await waitForConnected(page);
      await waitForSupplierCount(page, 3);

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.supplier-card');

      // Supplier cards visible
      const cardCount = await page.locator('.supplier-card').count();
      expect(cardCount).toBeGreaterThanOrEqual(3);

      await context.close();
    });

    test('wallet view renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      await completeSetup(page, {
        name: 'Emma',
        postcode: '2500',
      });
      await waitForConnected(page);

      // Navigate to wallet
      await page.click('button:has-text("Wallet")');
      await expect(page.locator('.wallet-view')).toBeVisible();

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.wallet-view');

      // Key wallet elements visible
      await expect(page.locator('.wallet-balance')).toBeVisible();
      await expect(page.locator('.exchange-rate')).toBeVisible();
      await expect(page.locator('button:has-text("Faucet")')).toBeVisible();

      // Peg-in section visible and inputs reachable
      const pegInSection = page.locator('.peg-section').first();
      await expect(pegInSection).toBeVisible();
      await assertWithinViewport(page, '.peg-section');

      // Peg-in input usable
      const satsInput = pegInSection.locator('input[type="number"]');
      await expect(satsInput).toBeVisible();
      await satsInput.fill('100');
      await expect(pegInSection.locator('button:has-text("Deposit via Lightning")')).toBeEnabled();

      // Peg-out section visible
      const pegOutSection = page.locator('.peg-section').last();
      await expect(pegOutSection).toBeVisible();

      await context.close();
    });

    test('storefront view renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      await completeSetup(page, {
        name: 'Emma',
        postcode: '2500',
      });
      await waitForConnected(page);
      await waitForSupplierCount(page, 3);

      // Click into Gary's storefront
      const garyCard = page.locator('.supplier-card', { hasText: 'Gary' });
      await garyCard.locator('a:has-text("View Storefront")').click();
      await expect(page.locator('.storefront-view')).toBeVisible();

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.storefront-view');

      // Products render
      await expect(async () => {
        const count = await page.locator('.product-card').count();
        expect(count).toBeGreaterThanOrEqual(1);
      }).toPass({ timeout: 15_000 });
      await assertWithinViewport(page, '.product-card');

      // Order button reachable
      await expect(page.locator('.product-card').first().locator('button:has-text("Order")')).toBeVisible();

      await context.close();
    });

    test('order form renders without overflow', async ({ browser }) => {
      const context = await browser.newContext({ viewport: { width: vp.width, height: vp.height } });
      const page = await context.newPage();

      await completeSetup(page, {
        name: 'Emma',
        postcode: '2500',
      });
      await waitForConnected(page);
      await waitForSupplierCount(page, 3);

      // Navigate to Gary's storefront and open order form
      const garyCard = page.locator('.supplier-card', { hasText: 'Gary' });
      await garyCard.locator('a:has-text("View Storefront")').click();
      await expect(page.locator('.storefront-view')).toBeVisible();

      await expect(async () => {
        const count = await page.locator('.product-card').count();
        expect(count).toBeGreaterThanOrEqual(1);
      }).toPass({ timeout: 15_000 });

      await page.locator('.product-card').first().locator('button:has-text("Order")').click();
      await expect(page.locator('.order-form')).toBeVisible();

      await assertNoHorizontalOverflow(page);
      await assertWithinViewport(page, '.order-form');

      // Form inputs and submit button visible
      await expect(page.locator('.order-form input[type="number"]')).toBeVisible();
      await expect(page.locator('.order-form button:has-text("Place Order")')).toBeVisible();

      await context.close();
    });
  });
}

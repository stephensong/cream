import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Farmer\'s Market', () => {
  test('market from harness appears in markets view', async ({ page }) => {
    // Register as a user to see the nav
    await completeSetup(page, {
      name: 'MarketBrowser',
      postcode: '2000',
    });

    await waitForConnected(page);
    await waitForSupplierCount(page, 3);

    // Navigate to the Markets page
    await page.click('button:has-text("Markets")');

    // The harness deploys "Coffs Harbour Farmers Market" — check it appears
    const marketCard = page.locator('.market-card', { hasText: 'Coffs Harbour Farmers Market' });
    await expect(marketCard).toBeVisible({ timeout: 30_000 });
    // Harness creates market with Gary+Emma; node test Step 12 adds Iris
    await expect(marketCard.locator('.supplier-count')).toContainText('3 suppliers');

    // Click "View Market" to navigate to market detail view
    await marketCard.locator('a:has-text("View Market")').click();

    // Verify market detail view renders
    await expect(page.locator('.market-view')).toBeVisible();
    await expect(page.locator('.market-view h2')).toHaveText('Coffs Harbour Farmers Market');

    // Verify venue info
    await expect(page.locator('.market-info')).toContainText('Coffs Harbour Showground');

    // Verify participating suppliers are listed (Gary+Emma from harness, Iris from Step 12)
    await expect(page.locator('.supplier-chip', { hasText: 'Gary' })).toBeVisible();
    await expect(page.locator('.supplier-chip', { hasText: 'Emma' })).toBeVisible();
    await expect(page.locator('.supplier-chip', { hasText: 'Iris' })).toBeVisible();

    // Verify aggregated products from Gary and Emma are shown
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });
  });

  test('supplier sees which markets list them', async ({ page }) => {
    // Gary is listed at the harness market
    await completeSetup(page, {
      name: 'Gary',
      postcode: '2450',
      isSupplier: true,
      description: 'Fresh dairy products',
    });

    await waitForConnected(page);

    // Navigate to My Storefront
    await page.click('button:has-text("My Storefront")');
    await expect(page.locator('.supplier-dashboard')).toBeVisible();

    // Wait for market directory data to propagate
    await expect(
      page.locator('.supplier-markets', { hasText: 'Coffs Harbour Farmers Market' })
    ).toBeVisible({ timeout: 30_000 });
  });
});

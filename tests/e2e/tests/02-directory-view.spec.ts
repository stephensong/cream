import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Directory View', () => {
  test('shows harness suppliers with correct product counts', async ({ page }) => {
    // Register as a customer to see the directory without own entry
    await completeSetup(page, {
      name: 'DirectoryViewer',
      postcode: '2000',
    });

    await waitForConnected(page);

    // Wait for the test-harness suppliers to appear
    // Harness baseline: Gary (4 products), Emma (1 product), Iris (0 products)
    // No prior tests mutate products, so counts are exact.
    await waitForSupplierCount(page, 3);

    // Check each supplier card
    const garyCard = page.locator('.supplier-card', { hasText: 'Gary' });
    await expect(garyCard.locator('.product-count')).toHaveText('4 products', { timeout: 15_000 });

    const emmaCard = page.locator('.supplier-card', { hasText: 'Emma' });
    await expect(emmaCard.locator('.product-count')).toHaveText('1 products', { timeout: 15_000 });

    const irisCard = page.locator('.supplier-card', { hasText: 'Iris' });
    await expect(irisCard.locator('.product-count')).toHaveText('0 products', { timeout: 15_000 });
  });

  test('search filters suppliers by name', async ({ page }) => {
    await completeSetup(page, {
      name: 'Searcher',
      postcode: '2000',
    });

    await waitForConnected(page);
    await waitForSupplierCount(page, 3);

    // Record how many suppliers are visible before filtering
    const totalBefore = await page.locator('.supplier-card').count();

    // Type a search query
    await page.fill('input[placeholder="Search suppliers..."]', 'Gary');

    // Only Gary's card should remain visible
    await expect(page.locator('.supplier-card')).toHaveCount(1);
    await expect(page.locator('.supplier-card', { hasText: 'Gary' })).toBeVisible();

    // Clear search → all suppliers return
    await page.fill('input[placeholder="Search suppliers..."]', '');
    await expect(page.locator('.supplier-card')).toHaveCount(totalBefore, { timeout: 5_000 });
  });
});

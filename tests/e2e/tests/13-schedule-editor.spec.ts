import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Schedule Editor', () => {
  test('returning supplier can edit and save opening hours', async ({ page }) => {
    // Log in as Gary — a harness supplier already in the directory.
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

    // Wait for products to load
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(4);
    }).toPass({ timeout: 15_000 });

    // The "Opening Hours" section should be visible
    const openingSection = page.locator('.dashboard-section', { hasText: 'Opening Hours' });
    await expect(openingSection).toBeVisible();

    // Click "Edit Hours" to open the schedule editor
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    // Verify all 7 day rows are present
    await expect(page.locator('.schedule-day-row')).toHaveCount(7);

    const mondayRow = page.locator('.schedule-day-row').first();
    await expect(mondayRow.locator('.schedule-day-label')).toHaveText('Monday');

    // Sunday (last row) — ensure it starts closed for this test
    const sundayRow = page.locator('.schedule-day-row').last();
    await expect(sundayRow.locator('.schedule-day-label')).toHaveText('Sunday');
    const sundayRanges = await sundayRow.locator('.schedule-time-range').count();
    if (sundayRanges > 0) {
      // Remove existing Sunday hours to reset to closed state
      for (let i = sundayRanges - 1; i >= 0; i--) {
        await sundayRow.locator('button:has-text("×")').first().click();
      }
      await expect(sundayRow.locator('.schedule-closed-label')).toHaveText('(Closed)');
    } else {
      await expect(sundayRow.locator('.schedule-closed-label')).toHaveText('(Closed)');
    }

    // Add hours for Sunday: click "+ Add" on Sunday's row
    await sundayRow.locator('button:has-text("+ Add")').click();
    // A time range should appear (defaults to 8:00–17:00)
    await expect(sundayRow.locator('.schedule-time-range')).toHaveCount(1);

    // Save the schedule
    await page.click('button:has-text("Save Schedule")');

    // Editor should close, summary should reappear
    await expect(page.locator('.schedule-editor')).not.toBeVisible();
    await expect(page.locator('.schedule-summary')).toBeVisible();

    // The summary should now include Sunday
    await expect(page.locator('.schedule-summary')).toContainText('Sun');
  });
});

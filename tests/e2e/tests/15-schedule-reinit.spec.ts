import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Schedule Editor Re-initialization', () => {
  test('editor populates existing hours on first open', async ({ page }) => {
    // Log in as Gary — harness schedule: Mon–Fri 8–17, Sat 9–12, Sun closed
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

    // Wait for products to load (confirms storefront data arrived from network)
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(4);
    }).toPass({ timeout: 15_000 });

    // The schedule summary should show the harness schedule
    const openingSection = page.locator('.dashboard-section', { hasText: 'Opening Hours' });
    await expect(openingSection.locator('.schedule-summary')).toContainText('Mon');

    // ── First open: editor should show existing hours ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    // Monday (first row) should have a time range, NOT "(Closed)"
    const mondayRow = page.locator('.schedule-day-row').first();
    await expect(mondayRow.locator('.schedule-day-label')).toHaveText('Monday');

    const mondayRangeCount = await mondayRow.locator('.schedule-time-range').count();
    const mondayClosedCount = await mondayRow.locator('.schedule-closed-label').count();

    console.log(`First open — Monday: ${mondayRangeCount} time ranges, ${mondayClosedCount} closed labels`);

    // Dump all day rows for debugging
    for (let day = 0; day < 7; day++) {
      const row = page.locator('.schedule-day-row').nth(day);
      const label = await row.locator('.schedule-day-label').textContent();
      const ranges = await row.locator('.schedule-time-range').count();
      const closed = await row.locator('.schedule-closed-label').count();
      console.log(`  ${label}: ${ranges} ranges, ${closed > 0 ? '(Closed)' : 'open'}`);
      if (ranges > 0) {
        // Log the select values for the first range
        const firstRange = row.locator('.schedule-time-range').first();
        const selects = firstRange.locator('select');
        const startVal = await selects.first().inputValue();
        const endVal = await selects.last().inputValue();
        console.log(`    first range: start=${startVal} end=${endVal}`);
      }
    }

    // Monday MUST have at least one time range
    expect(mondayRangeCount).toBeGreaterThanOrEqual(1);

    // Saturday (6th row, index 5) should also have a range
    const saturdayRow = page.locator('.schedule-day-row').nth(5);
    await expect(saturdayRow.locator('.schedule-day-label')).toHaveText('Saturday');
    expect(await saturdayRow.locator('.schedule-time-range').count()).toBeGreaterThanOrEqual(1);

    // Sunday (last row) should be closed
    const sundayRow = page.locator('.schedule-day-row').last();
    await expect(sundayRow.locator('.schedule-day-label')).toHaveText('Sunday');
    await expect(sundayRow.locator('.schedule-closed-label')).toBeVisible();

    // Cancel the edit (no changes)
    await page.click('button:has-text("Cancel")');
    await expect(page.locator('.schedule-editor')).not.toBeVisible();
  });

  test('editor re-populates hours after save-close-reopen cycle', async ({ page }) => {
    // Log in as Gary
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

    // Wait for storefront data
    await expect(async () => {
      const count = await page.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(4);
    }).toPass({ timeout: 15_000 });

    const openingSection = page.locator('.dashboard-section', { hasText: 'Opening Hours' });
    await expect(openingSection.locator('.schedule-summary')).toContainText('Mon');

    // ── Open editor, save without changes ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    // Verify Monday has a range before saving
    const mondayBefore = page.locator('.schedule-day-row').first();
    const rangesBefore = await mondayBefore.locator('.schedule-time-range').count();
    console.log(`Before save — Monday ranges: ${rangesBefore}`);
    expect(rangesBefore).toBeGreaterThanOrEqual(1);

    // Save
    await page.click('button:has-text("Save Schedule")');
    await expect(page.locator('.schedule-editor')).not.toBeVisible();

    // Verify summary still shows schedule
    await expect(openingSection.locator('.schedule-summary')).toContainText('Mon');

    // ── Re-open editor: should still show the saved hours ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    const mondayAfter = page.locator('.schedule-day-row').first();
    const rangesAfter = await mondayAfter.locator('.schedule-time-range').count();
    const closedAfter = await mondayAfter.locator('.schedule-closed-label').count();
    console.log(`After reopen — Monday: ${rangesAfter} ranges, ${closedAfter > 0 ? '(Closed)' : 'open'}`);

    // Dump all rows again
    for (let day = 0; day < 7; day++) {
      const row = page.locator('.schedule-day-row').nth(day);
      const label = await row.locator('.schedule-day-label').textContent();
      const ranges = await row.locator('.schedule-time-range').count();
      const closed = await row.locator('.schedule-closed-label').count();
      console.log(`  ${label}: ${ranges} ranges, ${closed > 0 ? '(Closed)' : 'open'}`);
    }

    // Monday MUST still have time ranges after reopen
    expect(rangesAfter).toBeGreaterThanOrEqual(1);
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected } from '../helpers/wait-for-app';

test.describe('Schedule Editor Re-initialization', () => {
  test('editor populates existing hours on first open', async ({ page }) => {
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

    // ── Step 1: Add Monday hours (if not already present) and save ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    const mondayRow = page.locator('.schedule-day-row').first();
    await expect(mondayRow.locator('.schedule-day-label')).toHaveText('Monday');

    const existingRanges = await mondayRow.locator('.schedule-time-range').count();
    if (existingRanges === 0) {
      // Add a default range for Monday
      await mondayRow.locator('button:has-text("+ Add")').click();
      await expect(mondayRow.locator('.schedule-time-range')).toHaveCount(1);
    }

    // Save
    await page.click('button:has-text("Save Schedule")');
    await expect(page.locator('.schedule-editor')).not.toBeVisible();
    await expect(openingSection.locator('.schedule-summary')).toContainText('Mon');

    // ── Step 2: Re-open editor — Monday should still have the range ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    const mondayRowAfter = page.locator('.schedule-day-row').first();
    const rangesAfter = await mondayRowAfter.locator('.schedule-time-range').count();
    expect(rangesAfter).toBeGreaterThanOrEqual(1);

    // Cancel
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

    // ── Open editor, ensure Tuesday has hours, and save ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    const tuesdayRow = page.locator('.schedule-day-row').nth(1);
    await expect(tuesdayRow.locator('.schedule-day-label')).toHaveText('Tuesday');

    const tuesdayRanges = await tuesdayRow.locator('.schedule-time-range').count();
    if (tuesdayRanges === 0) {
      await tuesdayRow.locator('button:has-text("+ Add")').click();
    }
    expect(await tuesdayRow.locator('.schedule-time-range').count()).toBeGreaterThanOrEqual(1);

    // Save
    await page.click('button:has-text("Save Schedule")');
    await expect(page.locator('.schedule-editor')).not.toBeVisible();

    // Verify summary shows some hours (Tue may be grouped into "Mon–Fri")
    await expect(openingSection.locator('.schedule-summary')).toBeVisible();

    // ── Re-open editor: should still show the saved hours ──
    await openingSection.locator('button:has-text("Edit Hours")').click();
    await expect(page.locator('.schedule-editor')).toBeVisible();

    const tuesdayAfter = page.locator('.schedule-day-row').nth(1);
    const rangesAfter = await tuesdayAfter.locator('.schedule-time-range').count();
    expect(rangesAfter).toBeGreaterThanOrEqual(1);

    // Cancel
    await page.click('button:has-text("Cancel")');
    await expect(page.locator('.schedule-editor')).not.toBeVisible();
  });
});

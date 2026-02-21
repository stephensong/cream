import { Page, expect } from '@playwright/test';

/**
 * Navigate to the app and wait for WASM to load.
 * The app shows either `.user-setup` (not logged in) or `.app-header` (logged in).
 */
export async function waitForAppLoad(page: Page): Promise<void> {
  await page.goto('/');
  // WASM compilation + load can be slow; wait up to 30s for something to render
  await expect(
    page.locator('.user-setup, .app-header').first()
  ).toBeVisible({ timeout: 30_000 });
}

/**
 * Wait for the Freenet connection status to show "Connected".
 */
export async function waitForConnected(page: Page): Promise<void> {
  await expect(
    page.locator('.connection-status.connected')
  ).toBeVisible({ timeout: 15_000 });
}

/**
 * Wait until the directory shows at least `n` supplier cards.
 */
export async function waitForSupplierCount(page: Page, n: number): Promise<void> {
  await expect(async () => {
    const count = await page.locator('.supplier-card').count();
    expect(count).toBeGreaterThanOrEqual(n);
  }).toPass({ timeout: 20_000 });
}

import { Page, expect, Locator } from '@playwright/test';

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
 * Navigate to an explicit URL and wait for WASM to load.
 */
export async function waitForAppLoadAt(page: Page, url: string): Promise<void> {
  await page.goto(url);
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

/**
 * Wait for any Dioxus hot-reload "rebuilding" overlay to clear.
 * dx serve shows "Your app is being rebuilt" when a non-hot-reloadable change is detected.
 * The overlay resets all app state when the rebuild completes, so callers should
 * wait for it to disappear before interacting with the page.
 */
export async function waitForRebuildComplete(page: Page): Promise<void> {
  const overlay = page.getByText('Your app is being rebuilt');
  // If overlay is visible, wait for it to disappear (rebuild finishes)
  const isVisible = await overlay.isVisible().catch(() => false);
  if (isVisible) {
    await expect(overlay).not.toBeVisible({ timeout: 60_000 });
    // After rebuild, wait for the app to re-render
    await expect(
      page.locator('.app-header').first()
    ).toBeVisible({ timeout: 15_000 });
  }
}

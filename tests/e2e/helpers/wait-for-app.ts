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
  await dismissDxToast(page);
}

/**
 * Navigate to an explicit URL and wait for WASM to load.
 */
export async function waitForAppLoadAt(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await expect(
    page.locator('.user-setup, .app-header').first()
  ).toBeVisible({ timeout: 30_000 });
  await dismissDxToast(page);
}

/**
 * Wait for the Freenet connection badge in the nav bar to show "Connected".
 */
export async function waitForConnected(page: Page): Promise<void> {
  await expect(
    page.locator('.connection-badge.connected')
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
 * Dismiss the Dioxus dx-serve "Your app is being rebuilt" toast overlay.
 *
 * dx serve 0.7.3 has a bug where its WebSocket never sends a "build complete"
 * message to new clients, so the overlay stays forever. The toast JS exposes
 * window.closeDXToast() which we call to dismiss it.
 */
export async function dismissDxToast(page: Page): Promise<void> {
  await page.evaluate(() => {
    if (typeof (window as any).closeDXToast === 'function') {
      (window as any).closeDXToast();
    }
  });
}

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Order Decrements Quantity', () => {
  test('quantity_available decreases after an order on both tabs', async ({ browser }) => {
    const garyContext = await browser.newContext();
    const emmaContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const emmaPage = await emmaContext.newPage();

    // Gary registers as a supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      password: 'gary',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Emma registers as a customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
      password: 'emma',
    });
    await waitForConnected(emmaPage);

    // Wait for Emma to see the directory with suppliers
    await waitForSupplierCount(emmaPage, 3);

    // Emma navigates to Gary's storefront
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    await expect(emmaPage.locator('.storefront-view')).toBeVisible();
    await expect(emmaPage.locator('.storefront-view h2')).toHaveText('Gary');

    // Wait for products to load
    await expect(async () => {
      const count = await emmaPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    // Read the initial quantity from the first product
    const firstProduct = emmaPage.locator('.product-card').first();
    const qtyText = await firstProduct.locator('.quantity').textContent();
    const initialQty = parseInt(qtyText!.replace('Available: ', ''), 10);
    expect(initialQty).toBeGreaterThanOrEqual(2);

    // Emma orders 2 units
    await firstProduct.locator('button:has-text("Order")').click();
    await expect(emmaPage.locator('.order-form')).toBeVisible();
    await emmaPage.fill('.order-form input[type="number"]', '2');
    await emmaPage.click('.order-form button:has-text("Place Order")');

    // Verify order confirmation
    await expect(emmaPage.locator('.order-confirmation')).toBeVisible();
    await expect(emmaPage.locator('.order-confirmation h3')).toHaveText('Order Submitted!');

    // Go back to products to see updated quantity
    await emmaPage.click('button:has-text("Back to Products")');

    // Verify Emma's storefront view shows decremented quantity
    const expectedQty = initialQty - 2;
    const updatedFirstProduct = emmaPage.locator('.product-card').first();
    await expect(updatedFirstProduct.locator('.quantity')).toHaveText(`Available: ${expectedQty}`, { timeout: 10_000 });

    // Gary navigates to My Storefront and checks quantity
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    // Wait for Gary's dashboard to show the updated quantity
    await expect(async () => {
      const card = garyPage.locator('.product-card').first();
      const text = await card.textContent();
      expect(text).toContain(`Available: ${expectedQty}`);
    }).toPass({ timeout: 30_000 });

    await garyContext.close();
    await emmaContext.close();
  });
});

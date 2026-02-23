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
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Emma registers as a customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);

    // Wait for Emma to see the directory with suppliers
    await waitForSupplierCount(emmaPage, 3);

    // Emma navigates to Gary's storefront
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();

    await expect(emmaPage.locator('.storefront-view')).toBeVisible();
    await expect(emmaPage.locator('.storefront-view h2')).toHaveText('Gary');

    // Cumulative state: Gary has 6 products (4 harness + test-04 + test-06)
    await expect(async () => {
      const count = await emmaPage.locator('.product-card').count();
      expect(count).toBeGreaterThanOrEqual(6);
    }).toPass({ timeout: 15_000 });

    // Find a product with enough stock (>=2) to order from
    const allProducts = emmaPage.locator('.product-card');
    const productCount = await allProducts.count();
    let targetProduct = allProducts.first();
    let initialQty = 0;
    for (let i = 0; i < productCount; i++) {
      const card = allProducts.nth(i);
      const qtyText = await card.locator('.quantity').textContent();
      const qty = parseInt(qtyText!.replace('Available: ', ''), 10);
      if (qty >= 2) {
        targetProduct = card;
        initialQty = qty;
        break;
      }
    }
    expect(initialQty).toBeGreaterThanOrEqual(2);

    // Remember the product name before placing the order (card disappears after)
    const productName = await targetProduct.locator('h3').first().textContent();

    // Emma orders 2 units
    await targetProduct.locator('button:has-text("Order")').click();
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
    await expect(async () => {
      // Find the specific product card that has the expected decremented quantity
      const cards = emmaPage.locator('.product-card', { hasText: productName! });
      const count = await cards.count();
      let found = false;
      for (let i = 0; i < count; i++) {
        const qtyEl = await cards.nth(i).locator('.quantity').textContent();
        if (qtyEl === `Available: ${expectedQty}`) {
          found = true;
          break;
        }
      }
      expect(found).toBe(true);
    }).toPass({ timeout: 10_000 });

    // Gary navigates to My Storefront and checks quantity
    await garyPage.click('button:has-text("My Storefront")');
    await expect(garyPage.locator('.supplier-dashboard')).toBeVisible();

    // Wait for Gary's dashboard to show the updated quantity
    await expect(async () => {
      const cards = garyPage.locator('.product-card', { hasText: productName! });
      const count = await cards.count();
      let found = false;
      for (let i = 0; i < count; i++) {
        const text = await cards.nth(i).textContent();
        if (text?.includes(`Available: ${expectedQty}`)) {
          found = true;
          break;
        }
      }
      expect(found).toBe(true);
    }).toPass({ timeout: 30_000 });

    await garyContext.close();
    await emmaContext.close();
  });
});

import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Inbox Message', () => {
  test('send and receive inbox direct message', async ({ browser }) => {
    const garyContext = await browser.newContext();
    const emmaContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const emmaPage = await emmaContext.newPage();

    // Gary registers as supplier
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    // Emma registers as supplier (so she appears in Gary's recipient dropdown)
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
      isSupplier: true,
      description: 'Organic milk farm',
    });
    await waitForConnected(emmaPage);

    // Wait for directory to load with both suppliers
    await waitForSupplierCount(garyPage, 2);

    // Gary navigates to Messages
    await garyPage.click('button:has-text("Messages")');
    await expect(garyPage.locator('.messages-compose')).toBeVisible({ timeout: 15_000 });

    // Select Emma as recipient and verify selection took effect
    await garyPage.selectOption('.messages-compose select', 'Emma');
    await expect(garyPage.locator('.messages-compose select')).toHaveValue('Emma');

    // Type message body
    const messageBody = 'Hello Emma, do you have any raw milk available this week?';
    await garyPage.fill('.messages-compose textarea', messageBody);

    // Wait for Send Message button to be enabled (confirms form state is valid)
    const sendBtn = garyPage.locator('.messages-compose-footer button', { hasText: 'Send Message' });
    await expect(sendBtn).toBeEnabled({ timeout: 10_000 });

    // Click Send Message
    await sendBtn.click();

    // Verify sent message appears in Gary's list
    await expect(async () => {
      const sentItems = await garyPage.locator('.messages-item-sent').count();
      expect(sentItems).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 15_000 });

    const sentItem = garyPage.locator('.messages-item-sent').first();
    await expect(sentItem.locator('.messages-item-badge')).toContainText('Sent');
    await expect(sentItem.locator('.messages-item-body')).toContainText(messageBody);

    // Verify compose form cleared
    await expect(garyPage.locator('.messages-compose textarea')).toHaveValue('');

    // Emma navigates to Messages and waits for the inbox message
    await emmaPage.click('button:has-text("Messages")');
    await expect(emmaPage.locator('.messages-view')).toBeVisible({ timeout: 15_000 });

    // Wait for Emma to receive the message via inbox contract subscription
    await expect(async () => {
      const items = await emmaPage.locator('.messages-item').count();
      expect(items).toBeGreaterThanOrEqual(1);
    }).toPass({ timeout: 20_000 });

    const receivedItem = emmaPage.locator('.messages-item').first();
    await expect(receivedItem.locator('.messages-item-sender')).toContainText('Gary');
    await expect(receivedItem.locator('.messages-item-badge')).toContainText('DM');
    await expect(receivedItem.locator('.messages-item-body')).toContainText(messageBody);

    await garyContext.close();
    await emmaContext.close();
  });
});

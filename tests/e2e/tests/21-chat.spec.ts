import { test, expect } from '@playwright/test';
import { completeSetup } from '../helpers/setup-flow';
import { waitForConnected, waitForSupplierCount } from '../helpers/wait-for-app';

test.describe('Chat', () => {
  test('chat message input visible on storefront', async ({ browser }) => {
    const emmaContext = await browser.newContext();
    const emmaPage = await emmaContext.newPage();

    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);
    await waitForSupplierCount(emmaPage, 1);

    // Navigate to Gary's storefront
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    // Message textarea and buttons should be visible
    const messageInput = emmaPage.locator('.chat-invite-input textarea');
    await expect(messageInput).toBeVisible({ timeout: 10_000 });
    const sendMsgBtn = emmaPage.locator('.chat-start-btn').first();
    await expect(sendMsgBtn).toBeVisible();
    await expect(sendMsgBtn).toContainText('Send Message');
    // Button disabled when input is empty
    await expect(sendMsgBtn).toBeDisabled();

    // Request Chat button also visible
    const requestChatBtn = emmaPage.locator('.request-chat-btn');
    await expect(requestChatBtn).toBeVisible();
    await expect(requestChatBtn).toContainText('Request Chat');

    await emmaContext.close();
  });

  test('invite, auto-accept, and exchange messages', async ({ browser }) => {
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

    // Emma registers as customer
    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);
    await waitForSupplierCount(emmaPage, 1);

    // Emma navigates to Gary's storefront
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    // Wait for Request Chat button to be visible
    const requestChatBtn = emmaPage.locator('.request-chat-btn');
    await expect(requestChatBtn).toBeVisible({ timeout: 10_000 });

    // Emma types an invite message and clicks Request Chat
    await emmaPage.fill('.chat-invite-input textarea', 'Hi Gary, do you have fresh milk?');
    await expect(requestChatBtn).toBeEnabled({ timeout: 10_000 });
    await requestChatBtn.click();

    // Emma's chat panel auto-opens with her invite message + pending notice
    await expect(emmaPage.locator('.chat-panel')).toBeVisible({ timeout: 5_000 });
    await expect(emmaPage.locator('.chat-bubble.chat-sent')).toContainText('Hi Gary, do you have fresh milk?', { timeout: 5_000 });
    await expect(emmaPage.locator('.chat-pending-notice')).toBeVisible();

    // Gary sees the invite banner at the top of the page
    await expect(garyPage.locator('.chat-invite-banner')).toBeVisible({ timeout: 10_000 });
    await expect(garyPage.locator('.chat-invite-banner')).toContainText('Emma');
    await expect(garyPage.locator('.chat-invite-banner')).toContainText('Hi Gary, do you have fresh milk?');

    // Gary clicks "Open Chat" on the banner — invite is auto-accepted
    await garyPage.locator('.chat-invite-banner-btn').click();
    await expect(garyPage.locator('.chat-panel')).toBeVisible({ timeout: 5_000 });

    // Gary sees the invite message
    await expect(garyPage.locator('.chat-bubble.chat-received')).toContainText('Hi Gary, do you have fresh milk?', { timeout: 5_000 });

    // Gary can now send messages (input visible after auto-accept)
    await expect(garyPage.locator('.chat-input input')).toBeVisible({ timeout: 5_000 });

    // Banner should disappear after opening chat
    await expect(garyPage.locator('.chat-invite-banner')).not.toBeVisible({ timeout: 5_000 });

    // Emma's pending notice should disappear (session now active)
    await expect(emmaPage.locator('.chat-pending-notice')).not.toBeVisible({ timeout: 10_000 });
    await expect(emmaPage.locator('.chat-input input')).toBeVisible();

    // Emma sends a follow-up message
    await emmaPage.fill('.chat-input input', 'Hello Gary!');
    await emmaPage.press('.chat-input input', 'Enter');

    // Gary receives the message
    await expect(garyPage.locator('.chat-bubble.chat-received').last()).toContainText('Hello Gary!', { timeout: 10_000 });

    // Gary replies
    await garyPage.fill('.chat-input input', 'Hi Emma!');
    await garyPage.press('.chat-input input', 'Enter');

    // Emma receives the reply
    await expect(emmaPage.locator('.chat-bubble.chat-received')).toContainText('Hi Emma!', { timeout: 10_000 });

    await garyContext.close();
    await emmaContext.close();
  });

  test('send button delivers message', async ({ browser }) => {
    const garyContext = await browser.newContext();
    const emmaContext = await browser.newContext();

    const garyPage = await garyContext.newPage();
    const emmaPage = await emmaContext.newPage();

    // Setup both users
    await completeSetup(garyPage, {
      name: 'Gary',
      postcode: '2000',
      isSupplier: true,
      description: 'Fresh dairy products',
    });
    await waitForConnected(garyPage);

    await completeSetup(emmaPage, {
      name: 'Emma',
      postcode: '2500',
    });
    await waitForConnected(emmaPage);
    await waitForSupplierCount(emmaPage, 1);

    // Emma starts chat with Gary via Request Chat
    const garyCard = emmaPage.locator('.supplier-card', { hasText: 'Gary' });
    await garyCard.locator('a:has-text("View Storefront")').click();
    await expect(emmaPage.locator('.storefront-view')).toBeVisible();

    await emmaPage.fill('.chat-invite-input textarea', 'Quick question');
    const requestChatBtn = emmaPage.locator('.request-chat-btn');
    await expect(requestChatBtn).toBeEnabled({ timeout: 10_000 });
    await requestChatBtn.click();

    // Emma's panel auto-opens
    await expect(emmaPage.locator('.chat-panel')).toBeVisible({ timeout: 5_000 });

    // Gary sees banner, opens chat (auto-accepts)
    await expect(garyPage.locator('.chat-invite-banner')).toBeVisible({ timeout: 10_000 });
    await garyPage.locator('.chat-invite-banner-btn').click();

    // Wait for active state (input visible on both sides)
    await expect(emmaPage.locator('.chat-input input')).toBeVisible({ timeout: 10_000 });

    // Send button disabled when empty
    await expect(emmaPage.locator('.chat-send-btn')).toBeDisabled();

    // Type a message and click Send
    await emmaPage.fill('.chat-input input', 'Sent via button');
    await expect(emmaPage.locator('.chat-send-btn')).toBeEnabled();
    await emmaPage.locator('.chat-send-btn').click();

    // Emma sees her sent message
    await expect(emmaPage.locator('.chat-bubble.chat-sent').last()).toContainText('Sent via button', { timeout: 5_000 });

    // Gary receives it
    await expect(garyPage.locator('.chat-bubble.chat-received').last()).toContainText('Sent via button', { timeout: 10_000 });

    // Input cleared after send
    await expect(emmaPage.locator('.chat-input input')).toHaveValue('');
    await expect(emmaPage.locator('.chat-send-btn')).toBeDisabled();

    await garyContext.close();
    await emmaContext.close();
  });
});

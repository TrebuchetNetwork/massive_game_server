const { expect } = require('@playwright/test');
const { connectClient } = require('./gameClient');

async function createConnectedClients(browser, count, options = {}) {
  const clients = [];
  if (options.connectConcurrently) {
    for (let index = 0; index < count; index += 1) {
      const context = await browser.newContext(options.contextOptions || {});
      const page = await context.newPage();
      clients.push({ context, page });
    }
    await Promise.all(
      clients.map(({ page }, index) =>
        connectClient(page, {
          ...options,
          name: options.nameFactory ? options.nameFactory(index) : `Multi${index + 1}`,
        })
      )
    );
    return clients;
  }

  for (let index = 0; index < count; index += 1) {
    const context = await browser.newContext(options.contextOptions || {});
    const page = await context.newPage();
    await connectClient(page, {
      ...options,
      name: options.nameFactory ? options.nameFactory(index) : `Multi${index + 1}`,
    });
    clients.push({ context, page });
  }
  return clients;
}

async function closeAllClients(clients) {
  await Promise.all(clients.map(async ({ context }) => {
    if (!context) return;
    try {
      await context.close();
    } catch (error) {
      const message = error && error.message ? error.message : String(error || '');
      if (!message.includes('Failed to find context')) {
        throw error;
      }
    }
  }));
}

async function waitForPlayerVisibility(page, count, timeout = 60000) {
  await page.waitForFunction(
    (expectedCount) => Number(window.__e2e?.playerCount || 0) >= Number(expectedCount || 0),
    count,
    { timeout }
  );
}

async function sendMovement(page, direction = 'KeyW', durationMs = 500) {
  await page.keyboard.down(direction);
  await page.waitForTimeout(durationMs);
  await page.keyboard.up(direction);
}

async function fireAtPosition(page, canvasBox, timeout = 250) {
  const box = canvasBox || await page.locator('canvas').first().boundingBox();
  expect(box).toBeTruthy();
  const targetX = box.x + box.width * 0.5;
  const targetY = box.y + box.height * 0.5;
  await page.mouse.move(targetX, targetY);
  await page.mouse.down();
  await page.waitForTimeout(timeout);
  await page.mouse.up();
}

module.exports = {
  closeAllClients,
  createConnectedClients,
  fireAtPosition,
  sendMovement,
  waitForPlayerVisibility,
};

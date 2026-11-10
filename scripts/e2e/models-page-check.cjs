const assert = require('node:assert/strict');
const { chromium } = require('@playwright/test');

const BASE_URL = process.env.MODELS_BASE_URL || 'https://space.selfware.design';

(async () => {
  const browser = await chromium.launch();
  try {
    for (const [name, width, height] of [['desktop', 1440, 1000], ['mobile', 390, 844]]) {
      const page = await browser.newPage({ viewport: { width, height } });
      const errors = [];
      const consoleErrors = [];
      page.on('pageerror', error => errors.push(String(error)));
      page.on('console', message => {
        if (message.type() === 'error') consoleErrors.push(message.text());
      });
      await page.goto(`${BASE_URL}/models/index.html`, { waitUntil: 'domcontentloaded', timeout: 30000 });
      await page.locator('.standings').scrollIntoViewIfNeeded();
      await page.waitForTimeout(300);
      const result = await page.evaluate(() => {
        const cells = [...document.querySelectorAll('.standings__model')];
        const division = document.querySelector('.standings__division');
        return {
          cells: cells.length,
          missingSlugs: cells.filter(cell => !cell.querySelector('small')?.textContent.trim()).length,
          viewport: innerWidth,
          documentWidth: document.documentElement.scrollWidth,
          firstIdentity: cells[0]?.textContent.trim(),
          divisionClientWidth: division?.clientWidth,
          divisionScrollWidth: division?.scrollWidth,
          divisionOverflowX: division ? getComputedStyle(division).overflowX : null,
        };
      });
      await page.screenshot({ path: `/tmp/arena-models-${name}.png`, fullPage: false });
      console.log(name, JSON.stringify(result));
      assert.equal(result.cells, 160, `${name}: every track roster row must be present`);
      assert.equal(result.missingSlugs, 0, `${name}: every standings cell must include its model slug`);
      assert.ok(result.documentWidth <= result.viewport, `${name}: page overflows horizontally by ${result.documentWidth - result.viewport}px`);
      if (result.divisionScrollWidth > result.divisionClientWidth) {
        assert.equal(result.divisionOverflowX, 'auto', `${name}: wide standings must be scrollable inside their division`);
      }
      assert.deepEqual(errors, [], `${name}: page errors`);
      assert.deepEqual(consoleErrors, [], `${name}: console errors`);
    }
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});

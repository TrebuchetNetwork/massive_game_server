const { defineConfig } = require('@playwright/test');

const projects = [
  {
    name: 'chromium',
    use: {
      browserName: 'chromium',
    },
  },
];

if (process.env.PLAYWRIGHT_CROSS_BROWSER === '1') {
  projects.push(
    {
      name: 'firefox',
      use: {
        browserName: 'firefox',
      },
    },
    {
      name: 'webkit',
      use: {
        browserName: 'webkit',
      },
    }
  );
}

module.exports = defineConfig({
  testDir: './tests',
  timeout: 240000,
  expect: { timeout: 60000 },
  retries: process.env.CI ? 1 : 0,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: process.env.E2E_BASE_URL || 'http://127.0.0.1:19080',
    headless: true,
    viewport: { width: 1280, height: 720 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure'
  },
  projects,
  reporter: [
    ['list'],
    ['html', { outputFolder: 'playwright-report', open: 'never' }],
    ['junit', { outputFile: 'test-results/junit.xml' }],
  ]
});

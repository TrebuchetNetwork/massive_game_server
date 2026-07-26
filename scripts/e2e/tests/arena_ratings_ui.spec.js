const { test, expect } = require('@playwright/test');
const { registerServerLifecycle } = require('./helpers/serverLifecycle');

registerServerLifecycle(test);

const ratingsPayload = {
  ok: true,
  data: {
    schema_version: 1,
    active: true,
    status: 'active',
    season_id: 'weekly-2026-07-23-test',
    generated_at: '2026-07-23T20:00:00Z',
    ranking: {
      source: 'https://openrouter.ai/api/v1/models?sort=top-weekly',
      window: 'weekly',
      retrieved_at: '2026-07-23T19:00:00Z',
    },
    methodology: {
      prompt_sha256: 'a'.repeat(64),
      source_limit_bytes: 51200,
      modes: ['arena', 'ctf', 'koth', 'tdm'],
      seed_sets: [104729],
      team_size: 10,
      rounds: 1,
      personal_weight: 0.4,
      team_weight: 0.35,
      collaboration_weight: 0.25,
      duel_strategy_weight: 0.75,
      world_strategy_weight: 0.25,
      world_squad_size: 3,
      world_max_ticks: 600,
      collaboration_kind: 'team_context_v2_support_telemetry',
    },
    league: {
      format: 'weekly_continuous_v1',
      week_id: '2026-W30',
      frozen_at: '2026-07-20T00:00:00Z',
      epochs_completed: 2,
      total_seed_count: 8,
      points_by_rank: [1000, 700],
      standings_order: ['season_points', 'epoch_wins', 'overall_rating'],
      ledger_sha256: 'b'.repeat(64),
    },
    roster: [
      {
        rank: 1,
        provider_rank: 2,
        model_id: 'deepseek-test',
        model_name: 'DeepSeek: V4 Flash',
        provider_model: 'deepseek/deepseek-v4-flash',
        overall_rating: 91.2,
        world_rating: 94.8,
        strategy_rating: 92.1,
        personal_rating: 88.4,
        team_rating: 94.1,
        collaboration_rating: 92.3,
        evaluation_engagements: 1116,
        season_points: 1700,
        epochs_played: 2,
        integrity_status: 'verified_wasm',
      },
      {
        rank: 2,
        provider_rank: 1,
        model_id: 'mimo-test',
        model_name: 'Xiaomi: MiMo V2.5',
        provider_model: 'xiaomi/mimo-v2.5',
        overall_rating: 84.6,
        world_rating: 70,
        strategy_rating: 0,
        personal_rating: 90.2,
        team_rating: 81.5,
        collaboration_rating: 79.8,
        evaluation_engagements: 1116,
        season_points: 1400,
        epochs_played: 2,
        integrity_status: 'verified_wasm',
      },
    ],
  },
};

async function mockRatings(page) {
  await page.route('**/api/public/arena/ratings*', (route) => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify(ratingsPayload),
  }));
}

test('landing roster renders multidimensional verified season ratings', async ({ page }) => {
  await mockRatings(page);
  await page.goto('/index.html', { waitUntil: 'domcontentloaded' });

  await expect(page.locator('.roster__row--rated')).toHaveCount(2);
  await expect(page.locator('.roster__row--rated').first()).toContainText('V4 Flash');
  await expect(page.locator('.roster__row--rated').first()).toContainText('92.1');
  await expect(page.locator('[data-arena-status]')).toContainText('2026-W30 · 2 epochs');
  await expect(page.locator('.roster__row--rated').first()).toContainText('1700 pts');
  await expect(page.locator('.roster__row--rated').nth(1).locator('.roster__score[title="Strategy rating"] b')).toHaveText('0');
  await expect(page.locator('.roster__row--human')).toContainText('UNRANKED');
});

test('in-game ratings dialog exposes all five scores and remains viewport-safe', async ({ page }) => {
  await mockRatings(page);
  await page.goto('/client.html', { waitUntil: 'domcontentloaded' });

  await expect(page.locator('#arenaEvolutionChip')).toBeVisible();
  await expect(page.locator('[data-client-arena-season]')).toHaveText('LIVE');
  await expect(page.locator('#arenaEvolutionChip')).toContainText('1700 pts');
  await page.locator('#arenaEvolutionChip').click();
  await expect(page.locator('#arenaRatingsDialog')).toBeVisible();
  await expect(page.locator('.arena-rating-row')).toHaveCount(2);
  await expect(page.locator('.arena-rating-row').first()).toContainText('92.1');
  await expect(page.locator('.arena-rating-row').first()).toContainText('88.4');
  await expect(page.locator('.arena-rating-row').first()).toContainText('94.1');
  await expect(page.locator('.arena-rating-row').first()).toContainText('92.3');
  await expect(page.locator('.arena-rating-row').first()).toContainText('94.8');
  await expect(page.locator('.arena-rating-row').first()).toContainText('1700 pts');
  await expect(page.locator('.arena-rating-row').nth(1).locator('[aria-label="Strategy rating 0 out of 100"]')).toBeVisible();

  const bounds = await page.locator('.arena-ratings-dialog__panel').evaluate((panel) => {
    const rect = panel.getBoundingClientRect();
    return {
      left: rect.left,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
    };
  });
  expect(bounds.left).toBeGreaterThanOrEqual(0);
  expect(bounds.top).toBeGreaterThanOrEqual(0);
  expect(bounds.right).toBeLessThanOrEqual(bounds.viewportWidth);
  expect(bounds.bottom).toBeLessThanOrEqual(bounds.viewportHeight);
});

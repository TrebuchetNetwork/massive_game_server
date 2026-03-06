# E2E Tests

## Setup
```bash
cd scripts/e2e
npm install
npm run install-browsers
```

## Run
```bash
cd scripts/e2e
npm run test
```

## UI Surface Audit
```bash
cd scripts/e2e
E2E_SERVER_SKIP=1 E2E_BASE_URL=http://127.0.0.1:18080 E2E_WS_URL=ws://127.0.0.1:18080/ws npm run ui-audit
```

- Captures and validates core UI surfaces:
  - idle menu
  - settings menu
  - focus mode
  - hidden HUD menu-toggle state
  - mobile layout
  - connected HUD
  - connected scoreboard
  - connected settings
- Outputs screenshots and `report.json` under `artifacts/ui_audit/<run-id>/`.

## UI Coverage + Visual Flow (Playwright)
```bash
cd scripts/e2e
E2E_BASE_URL=http://127.0.0.1:18080 npx playwright test tests/ui_coverage_visual.spec.js --workers=1 --reporter=list
```

- Validates interactive UI flow (menu, settings, connect, scoreboard).
- Captures screenshots as test artifacts for visual inspection.
- Computes execution coverage for key client modules:
  - `client_logic/UIManager.js`
  - `client_logic/InputManager.js`
  - `client_logic/ConnectionManager.js`
  - `client_logic/WorldRenderer.js`

## Auth + Reconnect E2E
```bash
cd scripts/e2e
npx playwright test tests/auth_flow.spec.js tests/reconnect_flow.spec.js --workers=1 --reporter=list
```

- `auth_flow.spec.js` verifies OTP request, verify-code, authenticated gameplay join, and logout revocation.
- `reconnect_flow.spec.js` forces a disconnect and verifies the client reconnects without ghost entities or duplicate player sprites.

## Notes
- The test suite starts the Rust server by default using:
  `cargo run -p massive_game_server_core --bin massive_game_server_core`
- The client is loaded from `/client.html` (served from `static_client/`).
- To use an existing server, run with:
  `E2E_SERVER_SKIP=1 E2E_BASE_URL=http://127.0.0.1:18080 npm run test`
- To override the server command:
  `E2E_SERVER_CMD="path/to/server_binary" npm run test`
- For one-command audit + e2e from repo root, use:
  `./scripts/validate_ui.sh`

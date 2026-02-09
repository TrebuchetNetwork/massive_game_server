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

## Notes
- The test suite starts the Rust server by default using:
  `cargo run -p massive_game_server_core --bin massive_game_server_core`
- The client is loaded from `/client.html` (served from `static_client/`).
- To use an existing server, run with:
  `E2E_SERVER_SKIP=1 E2E_BASE_URL=http://127.0.0.1:8080 npm run test`
- To override the server command:
  `E2E_SERVER_CMD="path/to/server_binary" npm run test`
- For one-command audit + e2e from repo root, use:
  `./scripts/validate_ui.sh`

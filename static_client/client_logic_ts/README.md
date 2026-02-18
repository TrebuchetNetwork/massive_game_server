# TypeScript Migration (Client)

This folder is the staged TypeScript migration target for client-side logic.

Current scope:
- `network_indicator.ts` migrated from JS with strict TypeScript checks.
- `index.ts` exports TS-native modules.

Build commands:
- `npm install` (inside `/Users/ivo/massive_game_server/static_client`)
- `npm run build:ts-client`

Output:
- compiled files are emitted to `client_logic_ts_build/`.

Migration strategy:
1. Port utility modules first.
2. Port effects/audio/minimap modules next.
3. Swap runtime imports in `client.html` from `client_logic/*.js` to `client_logic_ts_build/*.js`.

# FlatBuffers Schema Policy

## Canonical source

Use `/Users/ivo/massive_game_server/protocol/schemas/game.fbs` as the single editable schema source.

## Required mirror

Keep `/Users/ivo/massive_game_server/server/schemas/game.fbs` byte-identical to the canonical schema.

## Enforcement

- Local/server build fails on schema drift (`/Users/ivo/massive_game_server/server/build.rs`).
- CI runs `/Users/ivo/massive_game_server/scripts/check_flatbuffers_consistency.sh`.

## Regeneration workflow

1. Edit `/Users/ivo/massive_game_server/protocol/schemas/game.fbs`.
2. Mirror it to `/Users/ivo/massive_game_server/server/schemas/game.fbs`.
3. Run:

```bash
cd /Users/ivo/massive_game_server/scripts
./generate_flatbuffers.sh
```

4. Validate:

```bash
cd /Users/ivo/massive_game_server
scripts/check_flatbuffers_consistency.sh
```

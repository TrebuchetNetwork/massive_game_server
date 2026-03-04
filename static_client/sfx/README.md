# Client SFX Assets

This directory contains runtime sound effects loaded by
`static_client/client_logic/effects_audio_runtime.js`.

## Regenerating event SFX

The following event sounds are generated from layered FFmpeg synth/noise chains:

- `bullet_whiz.wav`
- `dash_whoosh.wav`
- `dodge_whoosh.wav`
- `spawn_chime.wav`
- `flag_fanfare.wav`

Run:

```bash
./generate_event_sfx.sh
```

from this directory (`static_client/sfx`).

The generator is deterministic and can be re-run to refresh these assets without
changing the client runtime code.

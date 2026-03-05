# Massive Game Server (Project Trebuchet Core)

Welcome to the Massive Game Server project! This is a high-performance game server written in Rust, designed from the ground up to handle a massive number of concurrent players and AI-controlled entities. It utilizes WebRTC for real-time client-server communication and FlatBuffers for efficient data serialization. This server is a core component of the Trebuchet Network initiative, aimed at pushing the boundaries of large-scale multiplayer interactions.

## Features

* **High-Performance Core:** Built in Rust for speed and safety.
* **Massive Scalability Focus:** Architected to support hundreds to thousands of entities.
* **Real-time 2D Shooter Base:** Includes fundamental gameplay logic for a 2D shooter.
* **WebRTC Networking:** Leverages WebRTC data channels for low-latency communication.
* **Efficient Serialization:** Uses FlatBuffers for compact and fast data exchange.
* **AOI (Area of Interest) System:** For efficient state synchronization to clients.
* **Configurable Server Parameters:** Tick rate, player sharding, and thread pools can be tuned for performance.
* **Basic Bot System:** Capable of simulating AI-controlled players for testing and gameplay.
* **Static Web Client:** Includes an HTML/JavaScript client (`static_client/client.html`) using Pixi.js for testing and visualization.

## Prerequisites

Before you begin, ensure you have the following installed:

* **Rust:** `rustc 1.86.0` or newer. Install via [rustup.rs](https://rustup.rs/).
* **Cargo:** `cargo 1.86.0` or newer (comes with Rust).
* **FlatBuffers Compiler (`flatc`):** `flatc version 25.2.10` or newer.
    * macOS: `brew install flatbuffers`
    * Ubuntu/Debian: `sudo apt-get install flatbuffers-compiler`
    * Other: Visit the [FlatBuffers Website](https://google.github.io/flatbuffers/).
* **(Optional) Node.js & npm:** Required if you plan to modify client-side TypeScript and recompile.
* **(Optional) TypeScript Compiler (`tsc`):** `Version 5.8.3` or newer (`npm install -g typescript`). Needed for `scripts/generate_flatbuffers.sh` if you modify the schema and want to recompile the client-side TypeScript.

## Getting Started

Follow these steps to get the server up and running:

1.  **Clone the Repository:**
    ```bash
    git clone [https://github.com/TrebuchetNetwork/massive_game_server.git](https://github.com/TrebuchetNetwork/massive_game_server.git) 
    # Replace with the actual URL if different, e.g., trebuchet_network
    cd massive_game_server 
    ```

2.  **Build the Server:**
    The server is located in the `server` subdirectory.
    ```bash
    cd server
    cargo build --release
    ```
    * **Note on FlatBuffers:** The `server/build.rs` script automatically uses `flatc` to compile the canonical FlatBuffers schema (`protocol/schemas/game.fbs`) into Rust code during the build process. `server/schemas/game.fbs` is a required mirror and must stay byte-identical.

3.  **Run the Server:**
    After a successful build:
    ```bash
    cd server 
    # Ensure you are in the server directory if you navigated away
    cargo run --release
    ```
    The server will start and log its status, typically indicating it's listening on `ws://0.0.0.0:8080/ws` and serving static files from `http://0.0.0.0:8080/`.

4.  **Test with the Static Web Client:**
    * Open the `static_client/client.html` file (located in the root of the cloned repository, e.g., `massive_game_server/static_client/client.html`) in a modern web browser.
    * The client should provide an interface to connect to the WebSocket URL logged by the server (default: `ws://localhost:8080/ws`).

## Client-Side Schema Generation

The static web client (`static_client/`) uses JavaScript code generated from the FlatBuffers schema.
* The pre-generated JavaScript files are located in `static_client/generated_js/`.
* If you modify the FlatBuffers schema (`protocol/schemas/game.fbs`), you need to regenerate these client-side files. Keep `server/schemas/game.fbs` mirrored, then run:
    ```bash
    cd scripts
    ./generate_flatbuffers.sh
    ```
    This script uses `flatc` to generate TypeScript files and then (optionally, if `tsc` is installed) compiles them to JavaScript.
* Schema policy and enforcement details: `docs/flatbuffers_schema_policy.md`.

## Configuration

The primary server configuration can be found and modified in:
* `server/src/core/config.rs`

Key parameters include:
* `tick_rate`: The server's simulation frequency (e.g., 30 or 60 Hz).
* `num_player_shards`: For distributing player processing load.
* `max_players_per_match`: Maximum concurrent players/bots.
* `ThreadPoolConfig`: Defines the number of threads for various tasks (physics, networking, AI, etc.).
* `target_bot_count`: Default number of bots to spawn.

These are set to default values optimized for a 12-core development machine but should be tuned for your specific hardware and load requirements.

### Single-Machine Performance Profile

For high-density runs on one host, enable:

* `MGS_SINGLE_MACHINE_OPT=1`: Enables tighter join/broadcast budgets for backlog control.
* `MGS_CPU_AFFINITY=1`: Pins server thread pools to CPU cores.

Linux kernel/hugepage checklist and scripts:

* `docs/single_machine_optimization_checklist.md`
* `scripts/setup-system.sh --check` (or `--apply` as root)
* `scripts/monitor.sh <pid>`

### Internet/Mobile WebRTC Connectivity (ICE/TURN)

For real internet play (especially mobile networks), configure ICE/TURN explicitly:

* `MGS_DISABLE_STUN=1|0`: Disable default STUN bootstrap when set to `1`.
* `MGS_STUN_URLS`: Comma-separated STUN URLs (example: `stun:stun.l.google.com:19302`).
* `MGS_TURN_URLS`: Comma-separated TURN URLs (example: `turn:turn.example.com:3478?transport=udp,turns:turn.example.com:5349?transport=tcp`).
* `MGS_TURN_USERNAME`: TURN username.
* `MGS_TURN_CREDENTIAL`: TURN credential/password.
* `MGS_ICE_SERVERS`: Extra ICE entries, semicolon-separated, with format:
  * `urls_csv|username|credential`
  * Example: `turn:turn-a.example.com:3478?transport=udp|user|pass;turns:turn-a.example.com:5349?transport=tcp|user|pass`

The client can also override ICE server URLs via URL params:
* `?stun=stun:...`
* `?turn=turn:...`
* `?ice=stun:...;turn:...|user|pass`

For security, TURN credentials are not accepted from URL query params. Supply credentials using:
* `window.__MGS_TURN_CONFIG = { username: "...", credential: "..." }`
* `sessionStorage` keys `mgs_turn_username` and `mgs_turn_credential`

### Auth Store Redis Cache

Phone/SMS auth profiles can be persisted in Redis (with file fallback still enabled):

* `MGS_REDIS_URL`: Redis connection string (example: `redis://127.0.0.1:6379/`).
* `MGS_REDIS_AUTH_STORE_KEY`: Redis key for serialized auth store JSON (default: `mgs:auth:persistent_store`).

If Redis is unavailable, the server automatically falls back to local file persistence (`MGS_AUTH_STORE_PATH`).

### LLM Arena API (New)

Arena endpoints are now available for model ladder workflows:

* `POST /api/arena/models/register`
* `POST /api/arena/models/heartbeat`
* `POST /api/arena/models/upload_wasm`
* `GET /api/arena/models`
* `POST /api/arena/matches/queue`
* `POST /api/arena/matches/queue_round_robin`
* `POST /api/arena/matches/claim_next`
* `POST /api/arena/matches/execute_next`
* `GET /api/arena/matches/pending`
* `POST /api/arena/matches/report`
* `GET /api/arena/leaderboard`
* `GET /api/arena/overview`
* `GET /api/arena/worker/stats`
* `POST /api/arena/code/validate`
* `POST /api/arena/code/generate`
* `POST /api/arena/code/generate_and_compile`

Storage:

* `MGS_ARENA_STORE_PATH`: JSON persistence path (default: `data/arena_store.json`).
* `MGS_ARENA_WASM_DIR`: directory for per-model wasm files (`<model_id>.wasm`, default: `data/arena_bots`).
* `MGS_ARENA_BOT_FUEL_PER_TICK`: wasm fuel budget per tick (default: `1000000`).
* `MGS_ARENA_BOT_MAX_TICKS`: default arena sandbox ticks for `execute_next` (default: `600`).
* `MGS_ARENA_WASM_MAX_BYTES`: max accepted wasm upload size in bytes (default: `2097152`).
* `OPENROUTER_API_KEY`: optional API key used by code-generation scaffolding routes.
* `OPENROUTER_BASE_URL`: optional OpenRouter base URL override.
* `OPENROUTER_HTTP_REFERER`: optional `HTTP-Referer` header for OpenRouter requests.
* `OPENROUTER_APP_TITLE`: optional `X-Title` header for OpenRouter requests.

Human-priority slot controls:

* `MGS_HUMAN_PRIORITY_ENABLED`: enable immediate bot eviction for human joins when full (default: `true`).
* `MGS_RESERVED_HUMAN_SLOTS`: keep this many slots free from bot auto-fill (default: `2`).

Arena worker controls:

* `MGS_ARENA_WORKER_ENABLED`: auto-execute queued arena matches in background loop (default: `false`).
* `MGS_ARENA_WORKER_INTERVAL_MS`: polling interval for worker loop (default: `1000`).
* `MGS_ARENA_WORKER_MAX_TICKS`: optional max ticks per worker-executed match.

Web console:

* `http://<host>:<port>/arena.html`

### Feature Flags / A-B Controls

Operational feature flag endpoints:

* `GET /api/ops/feature-flags`
* `POST /api/ops/feature-flags/set`
* `POST /api/ops/feature-flags/evaluate`

Configuration:

* `MGS_FEATURE_FLAGS`: bootstrap flags from env (example: `exp_new_ui=1,exp_tail_policy=0`).
* `MGS_FEATURE_FLAG_STORE_PATH`: persistent JSON path (default: `data/feature_flags.json`).

### Static Assets / CDN-Ready Headers

The static file route now emits long-lived immutable cache headers for JS/CSS/WASM/media assets and no-store for HTML.
For CDN-specific CORS overrides:

* `MGS_CDN_ORIGIN`: Optional `Access-Control-Allow-Origin` value for static assets.

### Client Background Tab Throttling

`static_client/client.html` now auto-throttles render/update cadence in hidden tabs.
You can disable it per-client with:

* `?tab_throttle=0`

## Project Structure

A brief overview of the main directories:

* `/server`: Contains all the Rust server-side code.
    * `/server/src/core`: Fundamental types, constants, error handling, and configuration.
    * `/server/src/entities`: Logic for players, projectiles, and other game entities.
    * `/server/src/systems`: Core game systems like physics, AI, combat, and objectives.
    * `/server/src/world`: World partitioning, map generation, and spatial indexing.
    * `/server/src/network`: WebRTC signaling, data channel management, and network message handling.
    * `/server/src/concurrent`: Thread pools, concurrent data structures.
    * `/server/src/operational`: Monitoring, diagnostics, and tuning utilities.
    * `/protocol/schemas/game.fbs`: Canonical FlatBuffers schema defining the network protocol.
    * `/server/schemas/game.fbs`: Mirror copy used for compatibility checks.
    * `/server/src/main.rs`: The main entry point for the server application.
    * `/server/src/lib.rs`: The library crate root for `massive_game_server_core`.
* `/static_client`: Contains the HTML, JavaScript, and CSS for the static web client.
    * `/static_client/generated_js/`: JavaScript files auto-generated from `game.fbs` by `flatc`.
* `/scripts`: Utility shell scripts for tasks like generating FlatBuffers code.
* `/config`: (Currently empty) Intended for YAML configuration files for different environments.
* `/docs`: (Placeholder) For additional documentation.

## Contributing

Contributions are welcome! We aim to make this a community-driven effort to explore the limits of massive-scale simulations. Please look out for a `CONTRIBUTING.md` file for guidelines on how to contribute, report issues, and propose features.

For now, if you're participating in the Project Trebuchet competition, please follow the specific guidelines provided for that event.

## License

This project is licensed under the **MIT License**. See the `LICENSE` file in the repository for full details.

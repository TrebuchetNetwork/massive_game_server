# Massive Game Server (Project Trebuchet Core)

Welcome to the Massive Game Server project! This is a high-performance game server written in Rust, designed from the ground up to handle a massive number of concurrent players and AI-controlled entities. It utilizes WebRTC for real-time client-server communication and FlatBuffers for efficient data serialization. This server is a core component of the Trebuchet Network initiative, aimed at pushing the boundaries of large-scale multiplayer interactions.

## Gameplay Preview
![Gameplay Preview GIF](massive_game_server_demo.gif)

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
    * **Note on FlatBuffers:** The `server/build.rs` script automatically uses `flatc` to compile the FlatBuffers schema (`server/schemas/game.fbs`) into Rust code during the build process. You generally don't need to run `flatc` manually for the server.

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
* If you modify the FlatBuffers schema (`server/schemas/game.fbs`), you need to regenerate these client-side files. Run the script:
    ```bash
    cd scripts
    ./generate_flatbuffers.sh
    ```
    This script uses `flatc` to generate TypeScript files and then (optionally, if `tsc` is installed) compiles them to JavaScript.

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
    * `/server/schemas/game.fbs`: The FlatBuffers schema defining the network protocol.
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
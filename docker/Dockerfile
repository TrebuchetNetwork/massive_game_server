FROM rust:1.86-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    flatbuffers-compiler \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY server ./server
COPY protocol ./protocol
COPY flatbuffers ./flatbuffers
COPY static_client ./static_client

RUN cargo build --release -p massive_game_server_core --bin massive_game_server_core

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/massive_game_server_core /app/massive_game_server_core
COPY --from=builder /app/static_client /app/static_client

RUN mkdir -p /app/data /app/artifacts /app/server && \
    chown -R nobody:nogroup /app

USER nobody

ENV MGS_HOST=0.0.0.0
ENV MGS_PORT=8080
ENV MGS_DISABLE_STUN=1
ENV MGS_TARGET_BOT_COUNT=0
ENV RUST_LOG=massive_game_server_core=warn,warp=warn,webrtc=warn

EXPOSE 8080

HEALTHCHECK --interval=20s --timeout=4s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1

CMD ["/app/massive_game_server_core"]

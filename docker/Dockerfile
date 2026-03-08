FROM rust:1.88-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    cmake \
    ninja-build \
    g++ \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Debian bookworm ships flatc 2.x which is incompatible with flatbuffers-rs 25.x.
# Build flatc from the matching upstream tag so codegen matches Cargo.lock.
RUN git clone --depth 1 --branch v25.12.19 https://github.com/google/flatbuffers.git /tmp/flatbuffers && \
    cmake -G Ninja -S /tmp/flatbuffers -B /tmp/flatbuffers/build \
      -DCMAKE_BUILD_TYPE=Release \
      -DFLATBUFFERS_BUILD_TESTS=OFF && \
    cmake --build /tmp/flatbuffers/build --target flatc --parallel && \
    install /tmp/flatbuffers/build/flatc /usr/local/bin/flatc && \
    rm -rf /tmp/flatbuffers

COPY Cargo.toml Cargo.lock ./
COPY server ./server
COPY protocol ./protocol
COPY stress-client ./stress-client
COPY admin-tools ./admin-tools
COPY static_client ./static_client

RUN cargo build --release --locked --features jemalloc -p massive_game_server_core --bin massive_game_server_core

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libjemalloc2 \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/massive_game_server_core /app/massive_game_server_core
COPY --from=builder /app/static_client /app/static_client
COPY scripts/test_support/capture_sms.sh /app/scripts/test_support/capture_sms.sh

RUN mkdir -p /app/data /app/artifacts /app/server && \
    chmod +x /app/scripts/test_support/capture_sms.sh && \
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

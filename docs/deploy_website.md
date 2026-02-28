# Deploy Website + Game Server

This repo now includes a deployable website entry point at:
- `/index.html` (landing page)
- `/client.html` (main game client)
- `/ui-template.html` (UI prototype from the VOID STRIKE plan)
- `/index_legacy.html` (previous operations-focused landing page)
- `/healthz` (health endpoint)

For the production rollout on `game.trebuchet.network`, use:
- `docs/game_trebuchet_network_deploy.md`

## Option 1: Docker Compose (Recommended)

From repo root:

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

Check status:

```bash
docker compose -f docker/docker-compose.yml ps
curl -fsS http://127.0.0.1:8080/healthz
```

Open in browser:
- `http://<your-host>:8080/`
- `http://<your-host>:8080/client.html`

Stop:

```bash
docker compose -f docker/docker-compose.yml down
```

Or use helper script:

```bash
DEPLOY_MODE=docker ./scripts/deploy.sh up
DEPLOY_MODE=docker ./scripts/deploy.sh status
DEPLOY_MODE=docker ./scripts/deploy.sh logs
DEPLOY_MODE=docker ./scripts/deploy.sh down
DEPLOY_MODE=docker ./scripts/deploy.sh rollback
```

## Option 2: Native Binary

Build:

```bash
cargo build --release -p massive_game_server_core --bin massive_game_server_core
```

Run:

```bash
MGS_HOST=0.0.0.0 \
MGS_PORT=8080 \
MGS_DISABLE_STUN=1 \
MGS_TARGET_BOT_COUNT=0 \
RUST_LOG=massive_game_server_core=warn,warp=warn,webrtc=warn \
./target/release/massive_game_server_core
```

Or use helper script:

```bash
DEPLOY_MODE=native MGS_PORT=8080 ./scripts/deploy.sh up
```

## Production Notes

1. Put TLS reverse proxy in front (Nginx/Caddy/Cloudflare) so clients use `https://` + `wss://`.
2. Keep `MGS_HOST=0.0.0.0` in container/native deployment.
3. Persist `/app/data` if you use auth/arena stores.
4. Configure Docker secret files in `docker/secrets/` (see `docker/secrets/README.md`) and point `.env` to `*_SECRET_FILE` paths.

## Minimal Nginx Reverse Proxy

```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

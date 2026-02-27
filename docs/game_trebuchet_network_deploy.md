# Deploy to game.trebuchet.network

This is the implementation runbook for the new subdomain deployment.

## What was added

- Domain-ready reverse proxy: `docker/nginx.conf`
- Full Docker stack (server + nginx + redis + prometheus + grafana + node-exporter): `docker/docker-compose.yml`
- Monitoring configs:
  - `docker/monitoring/prometheus.yml`
  - `docker/monitoring/alerts.yml`
  - `docker/monitoring/grafana/datasources/prometheus.yml`
  - `docker/monitoring/grafana/dashboards/default.yml`
  - `docker/monitoring/grafana/dashboard-json/game-server.json`
- Deployment env template: `docker/.env.example`
- Updated landing site and imported plan UI assets under `static_client/`
- Imported planning docs from the zip under `docs/void_strike_plan/`

## 1. DNS

Create DNS records:

- `A game.trebuchet.network -> <server IPv4>`
- `AAAA game.trebuchet.network -> <server IPv6>` (optional)

## 2. TLS certs

Place certs in `docker/ssl/`:

- `docker/ssl/fullchain.pem`
- `docker/ssl/privkey.pem`

You can automate Let's Encrypt issuance with:

```bash
CERTBOT_EMAIL=ops@trebuchet.network ./scripts/provision_tls_cert.sh game.trebuchet.network
```

Notes:
- stop anything bound to port `80` before running the script
- certbot state is stored in `docker/certbot/`

## 3. Environment bootstrap

From repo root:

```bash
cp docker/.env.example docker/.env
```

Then edit `docker/.env`:

- set `GRAFANA_ADMIN_PASSWORD`
- confirm `MGS_ALLOWED_ORIGINS=https://game.trebuchet.network`
- set optional secrets like `OPENROUTER_API_KEY`

## 4. Validate config

```bash
DEPLOY_MODE=docker ./scripts/deploy.sh validate
```

This validates both Docker Compose config and Nginx syntax.

## 5. Start stack

```bash
DEPLOY_MODE=docker ./scripts/deploy.sh up
```

Check status:

```bash
DEPLOY_MODE=docker ./scripts/deploy.sh status
```

## 6. Enable auto-start on reboot (baremetal Linux)

```bash
./scripts/install_compose_service.sh
```

This installs and enables `massive-game-server.service`.

## 7. Verify endpoints

Run full public checks:

```bash
./scripts/verify_public_deploy.sh game.trebuchet.network
```

Manual checks:

- Health: `https://game.trebuchet.network/healthz`
- Readiness: `https://game.trebuchet.network/readyz`
- Game client: `https://game.trebuchet.network/client.html`
- Landing page: `https://game.trebuchet.network/index.html`
- UI prototype: `https://game.trebuchet.network/ui-template.html`
- Grafana (proxied): `https://game.trebuchet.network/grafana/`

Prometheus is bound locally only by default:

- `http://127.0.0.1:9091`

## 8. Rollback

```bash
DEPLOY_MODE=docker ./scripts/deploy.sh down
```

Then redeploy a known-good commit.

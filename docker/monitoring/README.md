# Monitoring Stack

This directory contains Prometheus + Grafana provisioning for the Docker deployment,
plus non-production Alertmanager/webhook wiring used by release-edge verification.

## Files

- `prometheus.yml`: scrape targets
- `alerts.yml`: baseline alert rules
- `alertmanager.yml`: local Alertmanager routing
- `alert_webhook.py`: simple webhook sink used by CI edge verification
- `grafana/datasources/prometheus.yml`: datasource provisioning
- `grafana/dashboards/default.yml`: dashboard provider
- `grafana/dashboard-json/game-server.json`: imported dashboard

## Access

- Prometheus: `http://127.0.0.1:9091`
- Alertmanager: `http://127.0.0.1:9093`
- Alert webhook sink: `http://127.0.0.1:18081/healthz`
- Grafana (direct): `http://127.0.0.1:3000`
- Grafana (proxied): `https://game.trebuchet.network/grafana/`

## Notes

- The game server exporter is enabled by `MGS_METRICS_ENABLED=true` and defaults to `127.0.0.1:9090` in Docker compose for least-privilege binding.
- If you run Prometheus in a separate container, override `MGS_METRICS_BIND_ADDR=0.0.0.0:9090` intentionally.
- Prometheus forwards alerts to the bundled Alertmanager service in Docker.
- Alertmanager routes `critical` and `warning` alerts separately, but both currently terminate at the non-production webhook sink.
- The webhook sink is intended for CI/non-production verification, not paging.

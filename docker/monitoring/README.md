# Monitoring Stack

This directory contains Prometheus + Grafana provisioning for the Docker deployment.

## Files

- `prometheus.yml`: scrape targets
- `alerts.yml`: baseline alert rules
- `grafana/datasources/prometheus.yml`: datasource provisioning
- `grafana/dashboards/default.yml`: dashboard provider
- `grafana/dashboard-json/game-server.json`: imported dashboard

## Access

- Prometheus: `http://127.0.0.1:9091`
- Grafana (direct): `http://127.0.0.1:3000`
- Grafana (proxied): `https://game.trebuchet.network/grafana/`

## Notes

- The game server exporter is enabled by `MGS_METRICS_ENABLED=true` and bound to `0.0.0.0:9090` inside the container.
- Alerts are evaluated by Prometheus only; no Alertmanager wiring is included in this baseline.

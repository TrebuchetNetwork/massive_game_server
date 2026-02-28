# Docker Secrets

Create these files before running `DEPLOY_MODE=docker ./scripts/deploy.sh up`:

- `docker/secrets/openrouter_api_key`
- `docker/secrets/grafana_admin_user`
- `docker/secrets/grafana_admin_password`

Each file should contain only the raw secret value (single line, no quotes).

These files are git-ignored by `docker/secrets/.gitignore`.

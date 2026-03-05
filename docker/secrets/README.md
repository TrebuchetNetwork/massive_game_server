# Docker Secrets

Create these files before running `DEPLOY_MODE=docker ./scripts/deploy.sh up`:

- `docker/secrets/openrouter_api_key`
- `docker/secrets/grafana_admin_user`
- `docker/secrets/grafana_admin_password`

Each file should contain only the raw secret value (single line, no quotes).

These files are git-ignored by `docker/secrets/.gitignore`.

Quick local bootstrap:

```bash
openssl rand -hex 16 > docker/secrets/openrouter_api_key
printf 'admin\n' > docker/secrets/grafana_admin_user
openssl rand -base64 32 | tr -d '\n' > docker/secrets/grafana_admin_password
```

For production, replace `openrouter_api_key` with a real OpenRouter key.

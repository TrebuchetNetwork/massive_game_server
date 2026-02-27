#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOMAIN="${DOMAIN:-${1:-game.trebuchet.network}}"
EMAIL="${CERTBOT_EMAIL:-${2:-}}"
CERTBOT_STAGING="${CERTBOT_STAGING:-0}"
CERTBOT_IMAGE="${CERTBOT_IMAGE:-certbot/certbot:latest}"
CERTBOT_DIR="$ROOT_DIR/docker/certbot"
SSL_DIR="$ROOT_DIR/docker/ssl"

usage() {
  cat <<EOF
Usage:
  CERTBOT_EMAIL=ops@example.com ./scripts/provision_tls_cert.sh [domain]
  ./scripts/provision_tls_cert.sh [domain] [email]

Environment:
  CERTBOT_EMAIL     Email for Let's Encrypt registration (required if arg 2 omitted)
  CERTBOT_STAGING   Set to 1 to use Let's Encrypt staging CA
  CERTBOT_IMAGE     Certbot container image (default: certbot/certbot:latest)

Notes:
  - Port 80 must be reachable from the internet for HTTP-01 validation.
  - Stop services binding :80/:443 before running this script.
  - Generated certs are copied to:
      docker/ssl/fullchain.pem
      docker/ssl/privkey.pem
EOF
}

if [[ -z "$EMAIL" ]]; then
  echo "error: missing email. Set CERTBOT_EMAIL or pass [email] as argument 2." >&2
  usage >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker is required but not installed." >&2
  exit 1
fi

if command -v lsof >/dev/null 2>&1; then
  if lsof -iTCP:80 -sTCP:LISTEN -Pn >/dev/null 2>&1; then
    echo "error: port 80 is already in use. Stop nginx/compose before requesting certs." >&2
    exit 1
  fi
fi

mkdir -p "$CERTBOT_DIR/conf" "$CERTBOT_DIR/work" "$CERTBOT_DIR/logs" "$SSL_DIR"

certbot_args=(
  certonly
  --standalone
  --non-interactive
  --agree-tos
  --email "$EMAIL"
  --domain "$DOMAIN"
  --keep-until-expiring
  --preferred-challenges http
)

if [[ "$CERTBOT_STAGING" == "1" ]]; then
  certbot_args+=(--staging)
fi

docker run --rm \
  -p 80:80 \
  -v "$CERTBOT_DIR/conf:/etc/letsencrypt" \
  -v "$CERTBOT_DIR/work:/var/lib/letsencrypt" \
  -v "$CERTBOT_DIR/logs:/var/log/letsencrypt" \
  "$CERTBOT_IMAGE" \
  "${certbot_args[@]}"

fullchain_src="$CERTBOT_DIR/conf/live/$DOMAIN/fullchain.pem"
privkey_src="$CERTBOT_DIR/conf/live/$DOMAIN/privkey.pem"

if [[ ! -f "$fullchain_src" || ! -f "$privkey_src" ]]; then
  echo "error: certbot completed but expected cert files were not found for domain '$DOMAIN'." >&2
  exit 1
fi

cp "$fullchain_src" "$SSL_DIR/fullchain.pem"
cp "$privkey_src" "$SSL_DIR/privkey.pem"
chmod 644 "$SSL_DIR/fullchain.pem"
chmod 600 "$SSL_DIR/privkey.pem"

echo "TLS certificates installed:"
echo "  $SSL_DIR/fullchain.pem"
echo "  $SSL_DIR/privkey.pem"
echo
echo "Next steps:"
echo "  DEPLOY_MODE=docker ./scripts/deploy.sh validate"
echo "  DEPLOY_MODE=docker ./scripts/deploy.sh up"

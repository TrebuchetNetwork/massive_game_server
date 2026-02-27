Place your TLS certificate and key files in this directory for nginx termination on `game.trebuchet.network`:

- `fullchain.pem`
- `privkey.pem`

For local testing only, you can generate a self-signed cert:

```bash
openssl req -x509 -nodes -newkey rsa:2048 \
  -keyout docker/ssl/privkey.pem \
  -out docker/ssl/fullchain.pem \
  -days 365 \
  -subj "/CN=game.trebuchet.network"
```

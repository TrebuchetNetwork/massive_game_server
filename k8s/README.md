# Kubernetes Manifests

Apply in order:

```bash
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/secret.yaml
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml
kubectl apply -f k8s/networkpolicy.yaml
kubectl apply -f k8s/hpa.yaml
kubectl apply -f k8s/pdb.yaml
kubectl apply -f k8s/ingress.yaml
```

Notes:
- `readinessProbe` uses `/readyz`
- `livenessProbe` uses `/healthz`
- Stage A is intentionally single-replica until shared persistence is externalized for auth, flags, replay, and backups.
- WebRTC gameplay requires UDP exposure. The manifests reserve ports `50000-50003/udp` and the deployment binds them directly with `hostPort` for honest browser smoke coverage.
- `configmap.yaml` and `secret.yaml` are first-class runtime inputs; do not hardcode production tokens or endpoints in `deployment.yaml`.
- `ingress.yaml` is repo-managed, but the ingress controller and TLS secret provisioning remain environment-specific responsibilities.
- `MGS_REDIS_URL` in `secret.yaml` enables the shared Redis-backed stores already supported by the server:
  auth, feature flags, arena state, live replay dispute metadata, and live replay match metadata.
- The ConfigMap pins the Redis store keys explicitly so Stage A and future Stage B deployments use the
  same key layout instead of relying on code defaults.
- Stage B is still not “flip replicas to 2 and call it done”:
  replay payload files and backup artifacts remain filesystem-backed, and `hostPort` UDP bindings also
  require deliberate multi-node scheduling or a different exposure strategy before a true multi-replica
  rollout is valid.

Automation:
- `.github/workflows/k8s-kind-gate.yml` provisions a disposable `kind` cluster with UDP port mappings, loads the locally built image, applies these manifests, patches smoke-safe env/resources, verifies rollout, runs browser smoke against the cluster path, then checks rollout restart and single-pod continuity.

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

Automation:
- `.github/workflows/k8s-kind-gate.yml` provisions a disposable `kind` cluster with UDP port mappings, loads the locally built image, applies these manifests, patches smoke-safe env/resources, verifies rollout, runs browser smoke against the cluster path, then checks rollout restart and single-pod continuity.

# Kubernetes Manifests

Apply in order:

```bash
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml
kubectl apply -f k8s/networkpolicy.yaml
kubectl apply -f k8s/hpa.yaml
kubectl apply -f k8s/pdb.yaml
```

Notes:
- `readinessProbe` uses `/readyz`
- `livenessProbe` uses `/healthz`
- ingress/TLS termination is expected to be configured separately
- `networkpolicy.yaml` allows ingress only on TCP/8080 for `app=massive-game-server` pods

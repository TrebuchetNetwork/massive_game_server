# Deployment Guide: game.trebuchet.network

A comprehensive guide for deploying the Massive Game Server (Space Shooter) to the `game.trebuchet.network` subdomain.

---

## Table of Contents

1. [Infrastructure Setup](#1-infrastructure-setup)
2. [Docker Deployment](#2-docker-deployment)
3. [Game Server Deployment](#3-game-server-deployment)
4. [Website Deployment](#4-website-deployment)
5. [Database Setup](#5-database-setup)
6. [Monitoring & Logging](#6-monitoring--logging)
7. [Scaling Guide](#7-scaling-guide)
8. [Troubleshooting](#8-troubleshooting)

---

## 1. Infrastructure Setup

### 1.1 Server Requirements

#### Minimum Requirements (Development/Small Scale)
| Resource | Specification |
|----------|---------------|
| CPU | 2 vCPU cores |
| RAM | 4 GB |
| Storage | 20 GB SSD |
| Bandwidth | 100 Mbps |
| OS | Ubuntu 22.04 LTS / Debian 12 |

#### Recommended Requirements (Production - 100-200 concurrent players)
| Resource | Specification |
|----------|---------------|
| CPU | 4-8 vCPU cores (high single-thread performance) |
| RAM | 8-16 GB |
| Storage | 50 GB NVMe SSD |
| Bandwidth | 1 Gbps |
| OS | Ubuntu 22.04 LTS / Debian 12 |

#### High-Scale Requirements (500+ concurrent players)
| Resource | Specification |
|----------|---------------|
| CPU | 16+ vCPU cores |
| RAM | 32+ GB |
| Storage | 100 GB NVMe SSD |
| Bandwidth | 10 Gbps |
| Load Balancer | Required |

### 1.2 Cloud Provider Recommendations

#### Primary Recommendation: Hetzner Cloud
```bash
# Best price/performance for game servers
# CX42 instance: 8 vCPUs, 16 GB RAM, ~$25/month
# CPX41 instance: 8 vCPUs (dedicated), 16 GB RAM, ~$35/month
```

#### Alternative Options

**DigitalOcean**
- Premium AMD/Intel droplets recommended
- Good global network
- Managed Kubernetes available

**AWS**
- Use c6i/c7g instances for compute-optimized workloads
- Consider Global Accelerator for low-latency connections
- Higher cost but excellent reliability

**Google Cloud Platform**
- c2-standard-4 or higher
- Premium tier networking
- Good for global deployments

**OVHcloud**
- Competitive pricing
- Good DDoS protection
- European focus

### 1.3 Domain/Subdomain Configuration

#### DNS Setup for game.trebuchet.network

```bash
# Add these DNS records at your domain registrar/DNS provider

# A Record - Point to your server IP
Type: A
Name: game
Value: <YOUR_SERVER_IP>
TTL: 300

# Optional: IPv6 support
Type: AAAA
Name: game
Value: <YOUR_SERVER_IPV6>
TTL: 300
```

#### Cloudflare Configuration (Recommended)

```bash
# 1. Add domain to Cloudflare
# 2. Update nameservers at registrar
# 3. Configure DNS records:

Type: A
Name: game
Value: <YOUR_SERVER_IP>
Proxy Status: DNS Only (initially) / Proxied (after SSL setup)
TTL: Auto

# 4. SSL/TLS Settings:
#    - Mode: Full (strict)
#    - Always Use HTTPS: ON
#    - Minimum TLS Version: 1.2

# 5. Speed Settings:
#    - Auto Minify: JS, CSS, HTML
#    - Brotli: ON
#    - Early Hints: ON
```

### 1.4 SSL Certificate Setup

#### Option A: Let's Encrypt with Certbot (Recommended)

```bash
# Install Certbot
sudo apt update
sudo apt install -y certbot

# Obtain certificate (standalone mode - nginx not running)
sudo certbot certonly --standalone -d game.trebuchet.network

# Or with nginx running (using webroot)
sudo certbot certonly --webroot -w /var/www/html -d game.trebuchet.network

# Certificates will be at:
# /etc/letsencrypt/live/game.trebuchet.network/fullchain.pem
# /etc/letsencrypt/live/game.trebuchet.network/privkey.pem

# Auto-renewal (usually set up automatically)
sudo certbot renew --dry-run
```

#### Option B: Cloudflare Origin Certificates

```bash
# 1. Go to Cloudflare Dashboard > SSL/TLS > Origin Server
# 2. Create Certificate
# 3. Download certificate and private key
# 4. Place in /etc/nginx/ssl/ on server

# Example paths:
# /etc/nginx/ssl/fullchain.pem
# /etc/nginx/ssl/privkey.pem
```

#### Option C: Self-Signed (Development Only)

```bash
# Create self-signed certificate
mkdir -p docker/ssl
openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
  -keyout docker/ssl/privkey.pem \
  -out docker/ssl/fullchain.pem \
  -subj "/CN=game.trebuchet.network"
```

---

## 2. Docker Deployment

### 2.1 Complete docker-compose.yml

```yaml
version: '3.8'

services:
  massive-game-server:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    image: massive-game-server:latest
    container_name: massive-game-server
    restart: unless-stopped
    ports:
      - "127.0.0.1:8080:8080"
    environment:
      # Core Settings
      MGS_HOST: "0.0.0.0"
      MGS_PORT: "8080"
      MGS_DISABLE_STUN: "1"
      MGS_TARGET_BOT_COUNT: "0"
      
      # Logging
      RUST_LOG: "massive_game_server_core=info,warp=warn,webrtc=warn"
      RUST_LOG_JSON: "true"
      
      # Game Settings
      MGS_MAX_PLAYERS: "200"
      MGS_TICK_RATE: "60"
      MGS_ARENA_TIMEOUT_SECS: "300"
      
      # AI/LLM Integration (optional)
      # OPENROUTER_API_KEY: "${OPENROUTER_API_KEY}"
      
      # WebRTC Settings
      MGS_WEBRTC_ICE_SERVERS: '[{"urls":"stun:stun.l.google.com:19302"}]'
      
      # Metrics
      MGS_METRICS_ENABLED: "true"
      MGS_METRICS_BIND: "0.0.0.0:9090"
    volumes:
      - mgs_data:/app/data
      - mgs_artifacts:/app/artifacts
      - ./logs:/app/logs
    deploy:
      resources:
        limits:
          cpus: "2"
          memory: 4G
        reservations:
          cpus: "1"
          memory: 1G
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8080/healthz"]
      interval: 20s
      timeout: 4s
      start_period: 20s
      retries: 3
    networks:
      - game-network

  redis:
    image: redis:7-alpine
    container_name: massive-game-server-redis
    restart: unless-stopped
    command: redis-server --appendonly yes --maxmemory 256mb --maxmemory-policy allkeys-lru
    volumes:
      - redis_data:/data
    deploy:
      resources:
        limits:
          memory: 512M
        reservations:
          memory: 128M
    networks:
      - game-network

  nginx:
    image: nginx:1.27-alpine
    container_name: massive-game-server-nginx
    restart: unless-stopped
    depends_on:
      - massive-game-server
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
      - ./ssl:/etc/nginx/ssl:ro
      - nginx_cache:/var/cache/nginx
    deploy:
      resources:
        limits:
          memory: 256M
        reservations:
          memory: 64M
    networks:
      - game-network

  # Prometheus for metrics collection
  prometheus:
    image: prom/prometheus:v2.50.0
    container_name: massive-game-server-prometheus
    restart: unless-stopped
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.console.libraries=/usr/share/prometheus/console_libraries'
      - '--web.console.templates=/usr/share/prometheus/consoles'
      - '--web.enable-lifecycle'
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus_data:/prometheus
    ports:
      - "127.0.0.1:9091:9090"
    networks:
      - game-network

  # Grafana for visualization
  grafana:
    image: grafana/grafana:10.3.0
    container_name: massive-game-server-grafana
    restart: unless-stopped
    environment:
      GF_SECURITY_ADMIN_PASSWORD: ${GRAFANA_ADMIN_PASSWORD:-admin}
      GF_USERS_ALLOW_SIGN_UP: "false"
      GF_SERVER_ROOT_URL: "https://game.trebuchet.network/grafana"
      GF_SERVER_SERVE_FROM_SUB_PATH: "true"
    volumes:
      - grafana_data:/var/lib/grafana
      - ./monitoring/grafana/dashboards:/etc/grafana/provisioning/dashboards:ro
      - ./monitoring/grafana/datasources:/etc/grafana/provisioning/datasources:ro
    ports:
      - "127.0.0.1:3000:3000"
    networks:
      - game-network

  # Node Exporter for system metrics
  node-exporter:
    image: prom/node-exporter:v1.7.0
    container_name: massive-game-server-node-exporter
    restart: unless-stopped
    volumes:
      - /proc:/host/proc:ro
      - /sys:/host/sys:ro
      - /:/rootfs:ro
    command:
      - '--path.procfs=/host/proc'
      - '--path.rootfs=/rootfs'
      - '--path.sysfs=/host/sys'
      - '--collector.filesystem.mount-points-exclude=^/(sys|proc|dev|host|etc)($$|/)'
    networks:
      - game-network

volumes:
  mgs_data:
  mgs_artifacts:
  redis_data:
  nginx_cache:
  prometheus_data:
  grafana_data:

networks:
  game-network:
    driver: bridge
```

### 2.2 Environment Variables

Create a `.env` file in your docker directory:

```bash
# .env file for game.trebuchet.network deployment

# Game Server Configuration
MGS_MAX_PLAYERS=200
MGS_TICK_RATE=60
MGS_ARENA_TIMEOUT_SECS=300

# Logging
RUST_LOG=massive_game_server_core=info,warp=warn,webrtc=warn
RUST_LOG_JSON=true

# AI Integration (optional)
OPENROUTER_API_KEY=your_api_key_here

# Database (if using external)
DATABASE_URL=postgresql://user:pass@localhost/mgs

# Grafana
GRAFANA_ADMIN_PASSWORD=your_secure_password_here

# SSL Certificate paths (if not using standard locations)
SSL_CERT_PATH=/etc/nginx/ssl/fullchain.pem
SSL_KEY_PATH=/etc/nginx/ssl/privkey.pem
```

### 2.3 Volume Mounts

```bash
# Create required directories
mkdir -p docker/ssl
mkdir -p docker/logs
mkdir -p docker/monitoring/grafana/dashboards
mkdir -p docker/monitoring/grafana/datasources
mkdir -p docker/data

# Set proper permissions
sudo chown -R 1000:1000 docker/logs
sudo chown -R 472:472 docker/grafana_data  # Grafana user
```

### 2.4 Network Configuration

The docker-compose.yml creates a dedicated bridge network:

```yaml
networks:
  game-network:
    driver: bridge
    ipam:
      config:
        - subnet: 172.20.0.0/16
```

---

## 3. Game Server Deployment

### 3.1 Build Process

#### Option A: Build on Server

```bash
# 1. Clone the repository
git clone https://github.com/TrebuchetNetwork/massive_game_server.git
cd massive_game_server

# 2. Build the Docker image
cd docker
docker-compose build --no-cache

# 3. Start the services
docker-compose up -d
```

#### Option B: Build in CI and Deploy

```bash
# GitHub Actions workflow (see .github/workflows/deploy.yml)
# Build and push to container registry, then deploy

# On server:
docker pull ghcr.io/trebuchetnetwork/massive-game-server:latest
docker-compose up -d
```

### 3.2 Configuration Files

#### Production Configuration (config/production.toml)

```toml
[server]
host = "0.0.0.0"
port = 8080
tick_rate = 60
max_players = 200

[game]
arena_timeout_secs = 300
matchmaking_timeout_secs = 30
bot_fill_delay_secs = 5

[webrtc]
disable_stun = false
ice_servers = [
    { urls = "stun:stun.l.google.com:19302" },
    { urls = "stun:stun1.l.google.com:19302" }
]

[logging]
level = "info"
format = "json"
output = "stdout"

[metrics]
enabled = true
bind = "0.0.0.0:9090"

[security]
cors_origins = ["https://game.trebuchet.network"]
rate_limit_requests = 100
rate_limit_window_secs = 60
```

### 3.3 Environment-Specific Settings

#### Development
```bash
RUST_LOG=debug
MGS_DISABLE_STUN=1
MGS_TARGET_BOT_COUNT=10
```

#### Staging
```bash
RUST_LOG=info
MGS_DISABLE_STUN=0
MGS_TARGET_BOT_COUNT=5
MGS_MAX_PLAYERS=50
```

#### Production
```bash
RUST_LOG=warn
MGS_DISABLE_STUN=0
MGS_TARGET_BOT_COUNT=0
MGS_MAX_PLAYERS=200
RUST_LOG_JSON=true
```

### 3.4 Log Management

```bash
# View logs
docker-compose logs -f massive-game-server

# View last 100 lines
docker-compose logs --tail=100 massive-game-server

# JSON log parsing
docker-compose logs -f massive-game-server | jq -r '."

# Log rotation (configure in docker-compose)
logging:
  driver: "json-file"
  options:
    max-size: "100m"
    max-file: "5"
```

---

## 4. Website Deployment

### 4.1 Static File Serving

The game server serves static files directly. The static_client directory contains:

```
static_client/
├── index.html          # Main game page
├── client.html         # Alternative client
├── css/
│   └── styles.css
├── js/
│   ├── game.js
│   ├── network.js
│   └── renderer.js
├── assets/
│   ├── sprites/
│   ├── sounds/
│   └── fonts/
└── wasm/
    └── game_client.wasm
```

### 4.2 CDN Configuration (Cloudflare)

```bash
# Page Rules for caching static assets

# Rule 1: Cache static assets
URL: game.trebuchet.network/static/*
Settings:
  - Cache Level: Cache Everything
  - Edge Cache TTL: 1 month
  - Browser Cache TTL: 1 day

# Rule 2: No caching for API/WebSocket
URL: game.trebuchet.network/ws*
Settings:
  - Cache Level: Bypass

# Rule 3: Cache HTML with short TTL
URL: game.trebuchet.network/*.html
Settings:
  - Cache Level: Cache Everything
  - Edge Cache TTL: 5 minutes
```

### 4.3 Caching Strategies

#### Nginx Cache Configuration

```nginx
# Add to nginx.conf http block
proxy_cache_path /var/cache/nginx levels=1:2 keys_zone=game_cache:10m 
                 max_size=1g inactive=60m use_temp_path=off;

# Add to server block
location ~* \.(js|css|png|jpg|jpeg|gif|ico|svg|woff|woff2)$ {
    proxy_pass http://game_server;
    proxy_cache game_cache;
    proxy_cache_valid 200 1d;
    proxy_cache_valid 404 1m;
    add_header X-Cache-Status $upstream_cache_status;
    
    # Client-side caching
    expires 1d;
    add_header Cache-Control "public, immutable";
}
```

### 4.4 Asset Optimization

```bash
# Build optimized assets
cd static_client

# Minify JavaScript
terser js/game.js -o js/game.min.js -c -m

# Optimize images
optipng -o7 assets/sprites/*.png
jpegoptim --strip-all assets/images/*.jpg

# Generate WebP versions
cwebp -q 85 assets/sprites/player.png -o assets/sprites/player.webp

# Build with hashes for cache busting
# (implement in your build pipeline)
```

---

## 5. Database Setup

### 5.1 Player Data Storage

#### Option A: PostgreSQL (Recommended for production)

```yaml
# Add to docker-compose.yml
  postgres:
    image: postgres:16-alpine
    container_name: massive-game-server-postgres
    restart: unless-stopped
    environment:
      POSTGRES_USER: mgs
      POSTGRES_PASSWORD: ${DB_PASSWORD}
      POSTGRES_DB: massive_game_server
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./init.sql:/docker-entrypoint-initdb.d/init.sql:ro
    ports:
      - "127.0.0.1:5432:5432"
    networks:
      - game-network
```

```sql
-- init.sql
CREATE TABLE IF NOT EXISTS players (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(32) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    last_login TIMESTAMP WITH TIME ZONE,
    total_score BIGINT DEFAULT 0,
    games_played INTEGER DEFAULT 0,
    games_won INTEGER DEFAULT 0,
    settings JSONB DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS matches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    ended_at TIMESTAMP WITH TIME ZONE,
    game_mode VARCHAR(32) NOT NULL,
    map_name VARCHAR(64),
    max_players INTEGER,
    winner_team INTEGER,
    duration_seconds INTEGER
);

CREATE TABLE IF NOT EXISTS player_matches (
    player_id UUID REFERENCES players(id),
    match_id UUID REFERENCES matches(id),
    team INTEGER,
    score INTEGER DEFAULT 0,
    kills INTEGER DEFAULT 0,
    deaths INTEGER DEFAULT 0,
    assists INTEGER DEFAULT 0,
    PRIMARY KEY (player_id, match_id)
);

CREATE INDEX idx_player_matches_player ON player_matches(player_id);
CREATE INDEX idx_player_matches_match ON player_matches(match_id);
CREATE INDEX idx_matches_started_at ON matches(started_at);
```

#### Option B: Redis (For session/cache data)

Already included in docker-compose.yml for:
- Session storage
- Rate limiting
- Leaderboard caching
- Real-time stats

### 5.2 Match History

```rust
// Example: Store match results
async fn store_match_result(
    db: &PgPool,
    match_data: &MatchResult,
) -> Result<(), sqlx::Error> {
    let match_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO matches (game_mode, map_name, max_players, winner_team, duration_seconds)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#
    )
    .bind(&match_data.game_mode)
    .bind(&match_data.map_name)
    .bind(match_data.max_players)
    .bind(match_data.winner_team)
    .bind(match_data.duration_seconds)
    .fetch_one(db)
    .await?;
    
    // Store player stats...
    Ok(())
}
```

### 5.3 Stats Tracking

```sql
-- Player statistics view
CREATE VIEW player_stats AS
SELECT 
    p.id,
    p.username,
    p.total_score,
    p.games_played,
    p.games_won,
    ROUND(p.games_won::numeric / NULLIF(p.games_played, 0) * 100, 2) as win_rate,
    COALESCE(SUM(pm.kills), 0) as total_kills,
    COALESCE(SUM(pm.deaths), 0) as total_deaths,
    ROUND(COALESCE(SUM(pm.kills), 0)::numeric / NULLIF(SUM(pm.deaths), 0), 2) as kdr
FROM players p
LEFT JOIN player_matches pm ON p.id = pm.player_id
GROUP BY p.id, p.username, p.total_score, p.games_played, p.games_won;

-- Leaderboard
CREATE VIEW leaderboard AS
SELECT 
    username,
    total_score,
    games_played,
    win_rate,
    kdr,
    RANK() OVER (ORDER BY total_score DESC) as rank
FROM player_stats
ORDER BY total_score DESC
LIMIT 100;
```

### 5.4 Backup Procedures

```bash
#!/bin/bash
# backup.sh - Run daily via cron

BACKUP_DIR="/backups/postgres"
DATE=$(date +%Y%m%d_%H%M%S)
DB_NAME="massive_game_server"
RETENTION_DAYS=7

# Create backup directory
mkdir -p $BACKUP_DIR

# Backup PostgreSQL
docker exec massive-game-server-postgres pg_dump -U mgs $DB_NAME | \
    gzip > $BACKUP_DIR/mgs_backup_$DATE.sql.gz

# Backup Redis
docker exec massive-game-server-redis redis-cli BGSAVE
docker cp massive-game-server-redis:/data/dump.rdb $BACKUP_DIR/redis_backup_$DATE.rdb

# Upload to S3 (optional)
aws s3 sync $BACKUP_DIR s3://your-backup-bucket/game-server-backups/

# Clean old backups
find $BACKUP_DIR -name "*.gz" -mtime +$RETENTION_DAYS -delete
find $BACKUP_DIR -name "*.rdb" -mtime +$RETENTION_DAYS -delete
```

Add to crontab:
```bash
# Daily backup at 3 AM
0 3 * * * /path/to/backup.sh >> /var/log/mgs_backup.log 2>&1
```

---

## 6. Monitoring & Logging

### 6.1 Prometheus/Grafana Setup

#### Prometheus Configuration (monitoring/prometheus.yml)

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

alerting:
  alertmanagers:
    - static_configs:
        - targets: []

rule_files: []

scrape_configs:
  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'game-server'
    static_configs:
      - targets: ['massive-game-server:9090']
    metrics_path: /metrics

  - job_name: 'node-exporter'
    static_configs:
      - targets: ['node-exporter:9100']

  - job_name: 'nginx'
    static_configs:
      - targets: ['nginx:80']
    metrics_path: /nginx_status
```

#### Grafana Dashboard

Create `monitoring/grafana/dashboards/game-server.json`:

```json
{
  "dashboard": {
    "title": "Game Server Metrics",
    "panels": [
      {
        "title": "Active Players",
        "type": "stat",
        "targets": [{
          "expr": "mgs_active_players"
        }]
      },
      {
        "title": "Tick Rate",
        "type": "graph",
        "targets": [{
          "expr": "mgs_tick_rate"
        }]
      },
      {
        "title": "Memory Usage",
        "type": "graph",
        "targets": [{
          "expr": "process_resident_memory_bytes{job=\"game-server\"}"
        }]
      },
      {
        "title": "WebSocket Connections",
        "type": "graph",
        "targets": [{
          "expr": "mgs_websocket_connections"
        }]
      }
    ]
  }
}
```

#### Grafana Datasource

Create `monitoring/grafana/datasources/prometheus.yml`:

```yaml
apiVersion: 1

datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
    editable: false
```

### 6.2 Log Aggregation

#### Option A: Loki + Grafana

```yaml
# Add to docker-compose.yml
  loki:
    image: grafana/loki:2.9.0
    container_name: massive-game-server-loki
    restart: unless-stopped
    volumes:
      - ./monitoring/loki-config.yml:/etc/loki/local-config.yaml:ro
      - loki_data:/loki
    ports:
      - "127.0.0.1:3100:3100"
    networks:
      - game-network

  promtail:
    image: grafana/promtail:2.9.0
    container_name: massive-game-server-promtail
    restart: unless-stopped
    volumes:
      - ./monitoring/promtail-config.yml:/etc/promtail/config.yml:ro
      - /var/log:/var/log:ro
      - /var/lib/docker/containers:/var/lib/docker/containers:ro
    networks:
      - game-network
```

#### Option B: ELK Stack (Elasticsearch, Logstash, Kibana)

```yaml
# For larger deployments
  elasticsearch:
    image: elasticsearch:8.12.0
    environment:
      - discovery.type=single-node
      - xpack.security.enabled=false
    volumes:
      - es_data:/usr/share/elasticsearch/data

  kibana:
    image: kibana:8.12.0
    ports:
      - "5601:5601"
```

### 6.3 Alert Configuration

#### Prometheus Alert Rules

```yaml
# monitoring/alerts.yml
groups:
  - name: game-server
    rules:
      - alert: GameServerDown
        expr: up{job="game-server"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Game server is down"
          
      - alert: HighMemoryUsage
        expr: process_resident_memory_bytes{job="game-server"} > 3000000000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High memory usage detected"
          
      - alert: LowTickRate
        expr: mgs_tick_rate < 50
        for: 30s
        labels:
          severity: warning
        annotations:
          summary: "Game tick rate below threshold"
```

#### Alertmanager Configuration

```yaml
# monitoring/alertmanager.yml
global:
  smtp_smarthost: 'smtp.gmail.com:587'
  smtp_from: 'alerts@trebuchet.network'

route:
  receiver: 'default'

receivers:
  - name: 'default'
    email_configs:
      - to: 'admin@trebuchet.network'
        auth_username: 'alerts@trebuchet.network'
        auth_password: '${SMTP_PASSWORD}'
    slack_configs:
      - api_url: '${SLACK_WEBHOOK_URL}'
        channel: '#game-server-alerts'
```

### 6.4 Health Checks

```bash
# Health check endpoint
curl https://game.trebuchet.network/healthz

# Expected response:
{
  "ok": true,
  "timestamp": "2024-01-15T10:30:00Z",
  "version": "1.0.0",
  "players": 42,
  "uptime_seconds": 86400
}

# Readiness check
curl https://game.trebuchet.network/ready

# Metrics endpoint
curl http://localhost:9090/metrics
```

---

## 7. Scaling Guide

### 7.1 Horizontal Scaling

#### Multiple Game Server Instances

```yaml
# docker-compose.scale.yml
version: '3.8'

services:
  game-server-1:
    image: massive-game-server:latest
    environment:
      MGS_PORT: 8081
      MGS_SHARD_ID: 1
    ports:
      - "127.0.0.1:8081:8081"
    
  game-server-2:
    image: massive-game-server:latest
    environment:
      MGS_PORT: 8082
      MGS_SHARD_ID: 2
    ports:
      - "127.0.0.1:8082:8082"
    
  game-server-3:
    image: massive-game-server:latest
    environment:
      MGS_PORT: 8083
      MGS_SHARD_ID: 3
    ports:
      - "127.0.0.1:8083:8083"

  nginx:
    image: nginx:alpine
    volumes:
      - ./nginx-scale.conf:/etc/nginx/nginx.conf:ro
    ports:
      - "80:80"
      - "443:443"
```

#### Load Balancer Configuration

```nginx
# nginx-scale.conf
upstream game_servers {
    least_conn;
    server game-server-1:8081;
    server game-server-2:8082;
    server game-server-3:8083;
    keepalive 64;
}

server {
    listen 443 ssl http2;
    
    location /ws {
        proxy_pass http://game_servers;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

### 7.2 Load Balancing

#### Session Affinity (Sticky Sessions)

```nginx
# For WebSocket connections, use IP hash
upstream game_servers {
    ip_hash;
    server game-server-1:8081;
    server game-server-2:8082;
}
```

#### Health-Based Routing

```nginx
upstream game_servers {
    zone upstream_game 64k;
    
    server game-server-1:8081 weight=5 max_fails=3 fail_timeout=30s;
    server game-server-2:8082 weight=5 max_fails=3 fail_timeout=30s;
    server game-server-3:8083 backup;
    
    keepalive 64;
}
```

### 7.3 Shard Configuration

```rust
// Server-side shard configuration
pub struct ShardConfig {
    pub shard_id: u32,
    pub total_shards: u32,
    pub max_players_per_shard: u32,
    pub shard_regions: Vec<Region>,
}

impl ShardConfig {
    pub fn get_shard_for_player(&self, player_id: &str) -> u32 {
        // Consistent hashing for shard assignment
        let hash = hash(player_id);
        hash % self.total_shards
    }
}
```

### 7.4 Performance Tuning

#### System-Level Tuning

```bash
# /etc/sysctl.conf

# Increase file descriptor limits
fs.file-max = 1000000

# TCP optimization for game servers
net.ipv4.tcp_tw_reuse = 1
net.ipv4.tcp_fin_timeout = 15
net.ipv4.tcp_keepalive_time = 300
net.ipv4.tcp_max_syn_backlog = 65536
net.ipv4.tcp_max_tw_buckets = 1440000
net.core.netdev_max_backlog = 65536
net.core.somaxconn = 65535

# UDP optimization (for WebRTC)
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
net.ipv4.udp_mem = 8388608 12582912 16777216
```

Apply settings:
```bash
sudo sysctl -p
```

#### Docker Resource Limits

```yaml
deploy:
  resources:
    limits:
      cpus: '4'
      memory: 8G
      pids: 10000
    reservations:
      cpus: '2'
      memory: 4G
  ulimits:
    nofile:
      soft: 100000
      hard: 100000
```

#### Rust Application Tuning

```toml
# Cargo.toml - Release optimizations
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true

[profile.release-with-debug]
inherits = "release"
debug = true
strip = false
```

---

## 8. Troubleshooting

### 8.1 Common Issues

#### Issue: Server won't start

```bash
# Check logs
docker-compose logs massive-game-server

# Check port conflicts
sudo netstat -tlnp | grep 8080

# Verify environment variables
docker-compose config

# Check disk space
df -h

# Check memory
free -h
```

#### Issue: WebSocket connections failing

```bash
# Test WebSocket endpoint
curl -i -N \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Host: game.trebuchet.network" \
  -H "Origin: https://game.trebuchet.network" \
  https://game.trebuchet.network/ws

# Check nginx error logs
docker-compose logs nginx

# Verify SSL certificate
openssl s_client -connect game.trebuchet.network:443 -servername game.trebuchet.network
```

#### Issue: High memory usage

```bash
# Monitor memory in real-time
watch -n 1 docker stats massive-game-server

# Check for memory leaks
valgrind --tool=massif ./massive_game_server_core

# Profile with heaptrack
heaptrack ./massive_game_server_core
```

#### Issue: Low tick rate / lag

```bash
# Check CPU usage
htop

# Profile with perf
perf record -g ./massive_game_server_core
perf report

# Check system load
uptime
cat /proc/loadavg
```

### 8.2 Debug Procedures

#### Enable Debug Logging

```bash
# Set debug log level
export RUST_LOG=debug
export RUST_BACKTRACE=1

# Run with backtrace
RUST_BACKTRACE=full ./massive_game_server_core
```

#### Network Debugging

```bash
# Capture network traffic
sudo tcpdump -i any -w game_traffic.pcap port 8080

# Analyze with Wireshark
wireshark game_traffic.pcap

# Check connection count
ss -tan | grep 8080 | wc -l
```

#### Performance Profiling

```bash
# CPU profiling
perf record --call-graph dwarf -p $(pgrep massive_game_server)
perf report

# Flamegraph
cargo flamegraph --bin massive_game_server_core

# Tracing
cargo run --features tracing --bin massive_game_server_core
```

### 8.3 Recovery Steps

#### Server Crash Recovery

```bash
#!/bin/bash
# recovery.sh

# 1. Stop all services
docker-compose down

# 2. Check for data corruption
docker volume ls | grep mgs

# 3. Restore from backup if needed
# ./restore.sh $(ls -t /backups/mgs_backup_*.sql.gz | head -1)

# 4. Restart services
docker-compose up -d

# 5. Verify health
for i in {1..30}; do
    if curl -fsS http://localhost:8080/healthz > /dev/null; then
        echo "Server is healthy"
        exit 0
    fi
    sleep 1
done
echo "Server failed to start"
exit 1
```

#### Database Corruption Recovery

```bash
# Stop services
docker-compose stop massive-game-server

# Backup current state
docker exec massive-game-server-postgres pg_dump -U mgs massive_game_server > emergency_backup.sql

# Restore from backup
gunzip < /backups/mgs_backup_20240115_030000.sql.gz | docker exec -i massive-game-server-postgres psql -U mgs

# Restart
docker-compose start
```

### 8.4 Support Contacts

```
Project: Trebuchet Network
Repository: https://github.com/TrebuchetNetwork/massive_game_server

Issues: https://github.com/TrebuchetNetwork/massive_game_server/issues
Discussions: https://github.com/TrebuchetNetwork/massive_game_server/discussions

Emergency Contacts:
- Primary: admin@trebuchet.network
- Secondary: devops@trebuchet.network

Slack: #game-server-support
Discord: discord.gg/trebuchet
```

---

## Appendix A: Quick Reference

### Docker Commands

```bash
# Start services
docker-compose up -d

# View logs
docker-compose logs -f

# Restart service
docker-compose restart massive-game-server

# Update image
docker-compose pull && docker-compose up -d

# Scale service
docker-compose up -d --scale massive-game-server=3

# Clean up
docker-compose down -v
docker system prune -a
```

### Useful Scripts

```bash
# deploy.sh - Full deployment
#!/bin/bash
set -e

git pull
docker-compose build
docker-compose up -d
docker-compose ps
```

```bash
# status.sh - Check server status
#!/bin/bash
echo "=== Container Status ==="
docker-compose ps

echo "=== Health Check ==="
curl -s http://localhost:8080/healthz | jq

echo "=== Resource Usage ==="
docker stats --no-stream
```

---

## Appendix B: Security Checklist

- [ ] SSL certificates configured and auto-renewing
- [ ] Firewall rules configured (only 80, 443 open)
- [ ] Docker containers running as non-root user
- [ ] Secrets stored in environment variables, not in code
- [ ] Regular security updates applied
- [ ] DDoS protection enabled (Cloudflare/AWS Shield)
- [ ] Rate limiting configured
- [ ] CORS origins properly restricted
- [ ] Security headers configured in nginx
- [ ] Regular backups tested
- [ ] Monitoring and alerting enabled
- [ ] Incident response plan documented

---

*Last updated: 2024*
*Version: 1.0*

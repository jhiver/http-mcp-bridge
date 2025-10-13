# SaraMCP Deployment Guide

## Overview

This guide walks through deploying SaraMCP to the atlantic server using Docker, with automatic deployment via GitHub webhooks.

## Prerequisites

- SSH access to root@atlantic
- GitHub repository for saramcp
- Two domain names configured:
  - `saramcp.yourdomain.com` (main application)
  - `saramcp-webhook.yourdomain.com` (webhook endpoint)

## Phase 1: Local Docker Testing ✓

Files created:
- `Dockerfile` - Multi-stage build (Rust compilation + runtime)
- `docker-compose.yml` - Container configuration
- `.env.production` - Production environment template
- `webhook.js` - GitHub webhook listener
- `nginx-saramcp.conf` - Main app nginx config
- `nginx-saramcp-webhook.conf` - Webhook nginx config

Test locally:
```bash
docker compose build
docker compose up
```

Visit: http://localhost:3012

## Phase 2: Setup Deployment Directory on Server

```bash
# SSH into server
ssh root@atlantic

# Clone repository
cd /opt
git clone https://github.com/yourusername/saramcp.git
cd saramcp

# Create production .env file
cp .env.production .env
nano .env

# Update these values:
# - SESSION_SECRET (generate with: openssl rand -hex 32)
# - SARAMCP_MASTER_KEY (generate with: openssl rand -base64 32)

# Create data directory for SQLite
mkdir -p data

# Test Docker build on server (will take 5-10 minutes)
docker compose build

# Start container
docker compose up -d

# Check logs
docker logs -f saramcp

# Test locally
curl http://localhost:3012
```

## Phase 3: Setup Webhook Service

```bash
# Create webhook directory
mkdir -p /opt/saramcp-webhook
cd /opt/saramcp-webhook

# Copy webhook.js from repo
cp /opt/saramcp/webhook.js .

# Install node (if not already installed)
which node || apk add nodejs npm

# Test webhook script
WEBHOOK_SECRET=test123 node webhook.js
# Press Ctrl+C to stop

# Create systemd service
cat > /etc/systemd/system/saramcp-webhook.service <<'EOF'
[Unit]
Description=GitHub Webhook for SaraMCP
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/saramcp-webhook
Environment="WEBHOOK_SECRET=CHANGE_THIS_SECRET"
ExecStart=/usr/bin/node /opt/saramcp-webhook/webhook.js
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Generate strong webhook secret
openssl rand -hex 32

# Edit the service file with actual secret
nano /etc/systemd/system/saramcp-webhook.service

# Enable and start service
systemctl daemon-reload
systemctl enable saramcp-webhook
systemctl start saramcp-webhook
systemctl status saramcp-webhook

# Check logs
journalctl -u saramcp-webhook -f

# Test webhook locally
curl -X POST http://localhost:3760/webhook
# Should return "Forbidden" (no signature)
```

## Phase 4: Configure Nginx

⚠️ **IMPORTANT**: Do not modify existing nginx configs! We're only adding new ones.

```bash
# Copy nginx configs from repo
cp /opt/saramcp/nginx-saramcp.conf /etc/nginx/sites-available/saramcp.yourdomain.com
cp /opt/saramcp/nginx-saramcp-webhook.conf /etc/nginx/sites-available/saramcp-webhook.yourdomain.com

# Update domain names in the configs
nano /etc/nginx/sites-available/saramcp.yourdomain.com
nano /etc/nginx/sites-available/saramcp-webhook.yourdomain.com

# Test nginx config
nginx -t

# Enable sites
ln -s /etc/nginx/sites-available/saramcp.yourdomain.com /etc/nginx/sites-enabled/
ln -s /etc/nginx/sites-available/saramcp-webhook.yourdomain.com /etc/nginx/sites-enabled/

# Reload nginx
systemctl reload nginx

# Test locally
curl -H "Host: saramcp.yourdomain.com" http://localhost
curl -H "Host: saramcp-webhook.yourdomain.com" http://localhost/webhook
```

## Phase 5: Update Cloudflare Tunnel

```bash
# Backup current config
cp /root/.cloudflared/config-backend.yml /root/.cloudflared/config-backend.yml.backup

# Edit config
nano /root/.cloudflared/config-backend.yml

# Add these two entries BEFORE the final "- service: http_status:404" line:
#   - hostname: saramcp.yourdomain.com
#     service: http://127.0.0.1:80
#   - hostname: saramcp-webhook.yourdomain.com
#     service: http://127.0.0.1:80

# Restart cloudflared
systemctl restart cloudflared-backend
systemctl status cloudflared-backend

# Check logs
journalctl -u cloudflared-backend -f

# Test from external machine
curl https://saramcp.yourdomain.com
curl https://saramcp-webhook.yourdomain.com/webhook
```

## Phase 6: Configure GitHub Webhook

1. Go to your GitHub repository settings
2. Navigate to **Settings** → **Webhooks** → **Add webhook**
3. Configure:
   - **Payload URL**: `https://saramcp-webhook.yourdomain.com/webhook`
   - **Content type**: `application/json`
   - **Secret**: Use the WEBHOOK_SECRET from step 3
   - **Which events**: Just the push event
   - **Active**: ✓
4. Click **Add webhook**
5. GitHub will send a test ping

### Test Auto-Deployment

```bash
# On your local machine, make a small change
cd /Users/jhiver/saramcp
echo "# Test deployment" >> DEPLOYMENT_GUIDE.md
git add DEPLOYMENT_GUIDE.md
git commit -m "Test: Verify auto-deployment"
git push origin master

# On the server, watch the webhook logs
ssh root@atlantic
journalctl -u saramcp-webhook -f

# You should see:
# - "Received push to master, commit: <hash>"
# - "Starting Docker deployment..."
# - Build output
# - "Docker deployment successful"

# Watch docker logs
docker logs -f saramcp

# Check container is running
docker ps | grep saramcp
```

## Deployment Architecture

```
GitHub Push (master)
    ↓
GitHub Webhook
    ↓
https://saramcp-webhook.yourdomain.com/webhook
    ↓
Cloudflare Tunnel (port 443)
    ↓
Nginx (port 80)
    ↓
Webhook Service (Node.js, port 3760)
    ↓
Executes: git pull && docker compose build && docker compose up -d
    ↓
Docker Container (Rust app, port 8080 → exposed as 3012)
    ↓
Nginx (port 80)
    ↓
Cloudflare Tunnel
    ↓
https://saramcp.yourdomain.com
```

## Troubleshooting

### Container won't start
```bash
docker logs saramcp
docker ps -a | grep saramcp
```

### Webhook not triggering
```bash
# Check webhook service
systemctl status saramcp-webhook
journalctl -u saramcp-webhook -n 50

# Check GitHub webhook deliveries
# Go to: Settings → Webhooks → Recent Deliveries
```

### Nginx errors
```bash
nginx -t
systemctl status nginx
tail -f /var/log/nginx/error.log
```

### Cloudflare tunnel issues
```bash
systemctl status cloudflared-backend
journalctl -u cloudflared-backend -n 50
```

### Build failures
```bash
# Check disk space
df -h

# Clean docker cache
docker system prune -a

# Rebuild
cd /opt/saramcp
docker compose down
docker compose build --no-cache
docker compose up -d
```

## Maintenance

### View logs
```bash
# Application logs
docker logs -f saramcp

# Webhook logs
journalctl -u saramcp-webhook -f

# Nginx logs
tail -f /var/log/nginx/access.log
tail -f /var/log/nginx/error.log
```

### Manual deployment
```bash
cd /opt/saramcp
git pull origin master
docker compose down
docker compose build
docker compose up -d
```

### Backup database
```bash
# Database is in /opt/saramcp/data/saramcp.db
cp /opt/saramcp/data/saramcp.db /opt/saramcp/data/saramcp.db.backup-$(date +%Y%m%d)
```

### Update environment variables
```bash
cd /opt/saramcp
nano .env
docker compose restart
```

## Security Notes

- Webhook secret should be strong (32+ characters)
- SESSION_SECRET should be unique (64+ characters)
- SARAMCP_MASTER_KEY should be securely generated
- .env file contains secrets - never commit to git
- Webhook endpoint is public but signature-verified
- Application endpoint is public (add authentication if needed)

## Next Steps

After successful deployment:

1. Set up database backups (cron job)
2. Set up monitoring (health checks)
3. Configure email notifications for deployment failures
4. Set up staging environment (optional)
5. Configure automated database migrations

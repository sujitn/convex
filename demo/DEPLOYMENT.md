# Convex Demo Deployment Guide

This guide covers deploying the Convex demo application with:
- **Backend**: Fly.io (free tier)
- **Frontend**: Cloudflare Pages (free tier)

## Prerequisites

- [Fly.io CLI](https://fly.io/docs/hands-on/install-flyctl/)
- [Cloudflare account](https://dash.cloudflare.com/sign-up)
- Node.js 18+ and npm
- Rust 1.75+ (for local builds)

## Backend Deployment (Fly.io)

### 1. Install Fly CLI

```bash
# Windows (PowerShell)
iwr https://fly.io/install.ps1 -useb | iex

# macOS/Linux
curl -L https://fly.io/install.sh | sh
```

### 2. Login to Fly.io

```bash
fly auth login
```

### 3. Launch the App

From the repository root:

```bash
# First-time deployment
fly launch --name convex-pricing-api --region iad --no-deploy

# Review and deploy
fly deploy
```

### 4. Check Status

```bash
fly status
fly logs
```

### 5. Test the API

```bash
curl https://convex-pricing-api.fly.dev/health
```

### 6. Add Persistent Storage (Optional)

For persistent data storage:

```bash
fly volumes create convex_data --size 1 --region iad
```

Then uncomment the mounts section in `fly.toml`.

## Frontend Deployment (Cloudflare Pages)

### Method 1: Git Integration (Recommended)

1. Push your code to GitHub/GitLab
2. Go to [Cloudflare Pages](https://pages.cloudflare.com/)
3. Create a new project and connect your repository
4. Configure build settings:
   - **Build command**: `npm run build`
   - **Build output directory**: `dist`
   - **Root directory**: `demo/frontend`
5. Add environment variables:
   - `VITE_API_URL`: `https://convex-pricing-api.fly.dev`
   - `VITE_WS_URL`: `wss://convex-pricing-api.fly.dev/ws`
6. Deploy!

### Method 2: Direct Upload

```bash
# Build locally
cd demo/frontend
npm install
npm run build

# Install Wrangler CLI
npm install -g wrangler

# Login to Cloudflare
wrangler login

# Deploy
wrangler pages deploy dist --project-name convex-demo
```

### Environment Variables

Set these in Cloudflare Pages dashboard:

| Variable | Production Value |
|----------|-----------------|
| `VITE_API_URL` | `https://convex-pricing-api.fly.dev` |
| `VITE_WS_URL` | `wss://convex-pricing-api.fly.dev/ws` |

## Local Development

### Backend

```bash
# From repository root
cargo run -p convex-server
# Server runs at http://localhost:8080
```

### Frontend

```bash
cd demo/frontend
npm install
npm run dev
# Frontend runs at http://localhost:3000
# API calls are proxied to localhost:8080
```

## Cost Estimates

### Fly.io (Free Tier)
- 3 shared-cpu-1x VMs (256MB each)
- 160GB outbound data transfer
- **Cost**: $0/month for light usage

### Cloudflare Pages (Free Tier)
- 500 builds/month
- Unlimited bandwidth
- Unlimited requests
- **Cost**: $0/month

## Scaling

### Backend Scaling

```bash
# Scale to 2 instances
fly scale count 2

# Upgrade VM size
fly scale vm shared-cpu-2x --memory 512
```

### Custom Domain

```bash
# Fly.io
fly certs create api.yourdomain.com

# Cloudflare Pages
# Configure in Pages dashboard under Custom Domains
```

## Troubleshooting

### Backend Issues

```bash
# Check logs
fly logs --app convex-pricing-api

# SSH into container
fly ssh console

# Restart
fly apps restart convex-pricing-api
```

### Frontend Issues

- Check build logs in Cloudflare dashboard
- Verify environment variables are set correctly
- Check browser console for CORS errors

### CORS Errors

The server is configured to allow all origins. If you see CORS errors:
1. Verify the API URL is correct
2. Check that the server is running
3. Ensure you're using HTTPS in production

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Cloudflare Pages                         │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              React Frontend (Static)                 │    │
│  │  • Vite + TypeScript + Tailwind                     │    │
│  │  • recharts for visualizations                      │    │
│  │  • react-query for data fetching                    │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
                              │
                              │ HTTPS / WSS
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        Fly.io                                │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              convex-server (Rust)                    │    │
│  │  • REST API: /api/v1/*                              │    │
│  │  • WebSocket: /ws                                    │    │
│  │  • Health: /health                                   │    │
│  └─────────────────────────────────────────────────────┘    │
│                              │                               │
│                              ▼                               │
│  ┌─────────────────────────────────────────────────────┐    │
│  │         Persistent Volume (redb storage)             │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## API Endpoints Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/api/v1/curves` | GET/POST | List/create curves |
| `/api/v1/bonds` | GET/POST | List/create bonds |
| `/api/v1/quotes/:id` | GET | Get bond quote |
| `/api/v1/quote` | POST | Price single bond |
| `/api/v1/batch/price` | POST | Batch pricing |
| `/api/v1/etf/inav` | POST | Calculate ETF iNAV |
| `/api/v1/portfolio/analytics` | POST | Portfolio analytics |
| `/api/v1/stress/test` | POST | Run stress test |
| `/ws` | WS | WebSocket streaming |

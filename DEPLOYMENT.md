# Deployment Guide

## Installation

### CLI Setup

**MacOS**
```bash
brew install flyctl
```

**Linux**
```bash
curl -L https://fly.io/install.sh | sh
```

### Authentication

```bash
flyctl auth login
```

## Project Structure

```
.
├── server/           # Game server (WebSocket)
│   ├── Dockerfile.game-server
│   └── fly.toml
├── wallet/           # Wallet server (HTTP)
│   ├── Dockerfile.wallet-server
│   └── fly.toml
├── common/           # Shared code
├── create_schema.sql # Database schema
└── DEPLOYMENT.md
```

## Infrastructure Setup

### PostgreSQL Database

```bash
# Create a new PostgreSQL instance
flyctl postgres create

# Get connection string
flyctl postgres connect -a <database-app-name>

# Apply schema
psql <connection-string> < create_schema.sql
```

### Redis Instance

```bash
# Create a new Redis instance
flyctl redis create
```

## Environment Variables

**Required for both servers:**
```
# PostgreSQL connection
DATABASE_URL="postgres://..."

# Redis connection
REDIS_URL="redis://..."

# Logging
RUST_LOG="info"
```

## Deploying Services

### Game Server Deployment

```bash
# Navigate to server directory
cd server

# Set environment variables
flyctl secrets set DATABASE_URL="your-postgres-url"
flyctl secrets set REDIS_URL="your-redis-url"

# Launch the app (first-time setup)
flyctl launch

# Deploy
flyctl deploy
```

### Wallet Server Deployment

```bash
# Navigate to wallet directory
cd wallet

# Set environment variables
flyctl secrets set DATABASE_URL="your-postgres-url"
flyctl secrets set REDIS_URL="your-redis-url"

# Copy treasury keypair
# Make sure treasury-keypair.json is in the wallet directory

# Launch the app (first-time setup)
flyctl launch

# Deploy
flyctl deploy
```

## Monitoring & Management

### Logs

```bash
# Game Server logs
cd server && flyctl logs

# Wallet Server logs
cd wallet && flyctl logs
```

### Status

```bash
# Game Server status
cd server && flyctl status

# Wallet Server status
cd wallet && flyctl status
```

### Scaling

```bash
# Adjust instance count
flyctl scale count 2
```

### Common Commands

```bash
# View app information
flyctl info

# SSH into an instance
flyctl ssh console

# View recent deployments
flyctl deployments list

# View secrets
flyctl secrets list

# Monitor metrics
flyctl metrics
```

### Deployment Management

```bash
# List deployments
flyctl deployments list

# Rollback to previous version
flyctl deployments revert <deployment-id>
```

## Performance Monitoring

1. Use Fly.io metrics dashboard
2. Monitor WebSocket connection count
3. Track database performance
4. Watch Redis memory usage
5. Monitor API response times

## Backup Procedures

1. Database:
   ```bash
   # Backup PostgreSQL
   flyctl postgres backup
   ```

2. Configuration:
   - Keep copies of fly.toml files
   - Document all environment variables
   - Backup treasury keypair securely

## Troubleshooting

### Common Issues

1. **WebSocket Connection Issues**
   - Verify TLS settings in server/fly.toml
   - Check handlers: ["http", "tls", "http2"]
   - Verify port 3000 is accessible

2. **Database Connection Issues**
   - Check connection strings
   - Verify schema is properly initialized
   - Test connections locally first

3. **Build Issues**
   - Check Docker build logs
   - Verify Rust version compatibility
   - Ensure all dependencies are available

4. **Treasury Keypair Issues**
   - Verify treasury-keypair.json is present in wallet deployment
   - Check file permissions
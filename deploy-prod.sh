#!/bin/bash

# Meeting Co-Pilot Server Deployment Script
# Purpose: Pull latest changes, clean up, and restart the backend services (Backend, Celery, Redis).

set -e

# Configuration
PROJECT_ROOT="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
BACKEND_DIR="$PROJECT_ROOT/backend"
COMPOSE_FILE="$BACKEND_DIR/docker-compose.prod.yml"

cd "$PROJECT_ROOT"

echo "------------------------------------------------"
echo "🚀 REFRESHING AND DEPLOYING BACKEND SERVICES"
echo "------------------------------------------------"

# 1. Sync Code
echo "📥 Step 1: Updating code from origin/main..."
git pull origin main

# 2. Aggressive Cleanup
echo "🧹 Step 2: Removing irrelevant items for production server..."
# Remove Python caches
find . -type d -name "__pycache__" -exec rm -rf {} +
find . -type f -name "*.pyc" -delete
# Remove log files
find . -type f -name "*.log" -delete
# Clean up temp/test folders
rm -rf backend/.pytest_cache
rm -rf .aider.tags.cache.json

# If this is a dedicated backend server, we can remove the frontend code to save space
if [ -d "frontend" ]; then
    echo "🗑️ Removing frontend directory (not required for backend server)..."
    rm -rf frontend
fi

# Remove redundant docs
rm -rf docs
rm -rf meeting-copilot-docs
rm -f *.md

echo "✨ Cleanup complete."

# 3. Docker Deployment
echo "🔍 Step 3: Deploying with Docker Compose..."

if ! command -v docker &> /dev/null; then
    echo "❌ Error: Docker not found. Please install Docker first."
    exit 1
fi

cd "$BACKEND_DIR"

echo "🛑 Stopping existing containers..."
# Stop and remove containers defined in the compose file
docker compose -f docker-compose.prod.yml down --remove-orphans || true

# Stop any lingering container specifically named meeting-copilot-backend (from old manual runs)
docker stop meeting-copilot-backend 2>/dev/null || true
docker rm meeting-copilot-backend 2>/dev/null || true

# Check if ANY other process is using port 5167
if command -v lsof &> /dev/null; then
    NATIVE_PID=$(lsof -t -i :5167)
    if [ -n "$NATIVE_PID" ]; then
        echo "Found native process using port 5167 (PID: $NATIVE_PID). Killing it..."
        kill -9 $NATIVE_PID || true
    fi
fi

echo "🏗️ Building services (Backend, Celery, Redis)..."
docker compose -f docker-compose.prod.yml build

echo "🚀 Starting services..."
# Start in detached mode
docker compose -f docker-compose.prod.yml up -d

echo "✅ Services started successfully:"
docker compose -f docker-compose.prod.yml ps

echo "📝 to follow logs, run: cd backend && docker compose -f docker-compose.prod.yml logs -f"

echo "------------------------------------------------"
echo "🎉 Server Deployment Finished Successfully!"
echo "------------------------------------------------"

# Deploy FastAPI Auth API to Azure Web App

**Date**: 2026-03-30
**Status**: Approved
**Branch**: feature/mobile-app

## Problem

The mobile app's auth/cloud API (`api.iqcapture.app`) isn't deployed anywhere. The FastAPI backend runs locally but mobile devices can't reach `localhost`. The app needs a publicly accessible API endpoint for login, registration, meeting sync, transcription, and summarisation.

## Design

### Container Image

- **Source**: `backend/Dockerfile.app` (existing, production-ready)
- **Registry**: Caremanager Azure Container Registry (existing)
- **Image name**: `iqcapture-api`
- **Tag convention**: `latest` for now (manual push, no CI/CD)

### Azure Web App

- **Service**: Azure App Service — Web App for Containers (Linux)
- **Region**: UK South
- **SKU**: B1 (Basic, suitable for initial deployment)
- **Container source**: Caremanager ACR → `iqcapture-api:latest`
- **Port**: 5167 (set via `WEBSITES_PORT` application setting)
- **URL**: Default `iqcapture-api.azurewebsites.net` (no custom DNS)

### Application Settings (Environment Variables)

Configured via Azure Portal or CLI as JSON. Required:

| Variable | Source | Required |
|----------|--------|----------|
| `JWT_SECRET` | Existing `.env` | Yes |
| `MONGODB_URI` | Existing `.env` | Yes |
| `SENDGRID_API_KEY` | Existing `.env` | No (codes logged if absent) |
| `SENDGRID_FROM_EMAIL` | Existing `.env` | No |
| `CORS_ORIGINS` | Set to `*` initially | No |
| `DEEPGRAM_API_KEY` | Existing `.env` | No |
| `OPENAI_API_KEY` | Existing `.env` | No |
| `ANTHROPIC_API_KEY` | Existing `.env` | No |

### Mobile App Config Update

Update `mobile/src/services/config.ts` to point the production API URL at the Azure Web App default hostname instead of `api.iqcapture.app`.

### Health Check

The Web App health probe should hit `GET /health` (unauthenticated endpoint already in the FastAPI app).

## Out of Scope

- Whisper transcription server (stays local / not deployed)
- Custom DNS (`api.iqcapture.app` mapping)
- Azure Key Vault for secrets
- CI/CD pipeline (manual `docker push` for now)
- Staging slots
- Auto-scaling

## Rollback

Delete the Web App resource. No impact on existing infrastructure or the ACR.

# Cost Estimation: Meeting Co-Pilot (Production Audit)

**Date:** March 06, 2026
**Status:** Estimates based on **Audit of Actual Codebase**, API Pricing, & GCP Architecture.

---

## 1. Executive Summary

Our current architecture is built for **cost-efficiency and modularity**, leveraging Google's aggressive pricing for Gemini models as the core. The codebase supports multiple fallbacks (OpenAI, Anthropic, Groq, local models) providing flexibility of cost versus output quality.

The primary expenses derive from **Infrastructure (GCP)** and **Audio Processing (Deepgram/Groq)**, rather than core LLM text token generation.

*   **Cost Per 1-Hour Meeting (Base):** **~$0.04** (Live Transcript) to **$0.30** (Uploaded Audio Diarization).
*   **Audio Notes Generation:** **~$0.001–$0.010** additional spend for multimodal ingestion.
*   **Infrastructure Cost (Current Fixed):** **~$94.00 / mo** (GCP Compute, Cloud Run, DB).
*   **Infrastructure Cost (Proposed Serverless Migration):** **~$35.00 - $60.00 / mo** (Shifting to Cloud Tasks + Memorystore).
*   **Hidden Win:** Vector embeddings run locally (Free), avoiding API embedding costs entirely.

---

## 2. API & Model Economics (Unit Costs)

### A. Speech-to-Text (STT) & Diarization
*Code Audit: The application utilizes Groq for fast live streaming and Deepgram for processing audio files with speaker diarization.*

| Service | Model | Cost / Hour | Role in Codebase |
| :--- | :--- | :--- | :--- |
| **Groq API** | Whisper Large v3 | **$0.036** | **Live Streaming (`audio_pipeline.py`).** Ultra-low latency, cheap text. |
| **Deepgram** | Nova-2 | **$0.260** | **File Uploads/Diarization (`diarization.py`).** Used for offline speaker separation and high-accuracy processing. |

> **Optimization:** Live meetings only cost $0.04/hr. Relying heavily on Deepgram file uploads post-meeting increases STT costs to ~$0.30/hr.

### B. LLM Intelligence (Summaries, Chat, Logic)
*Code Audit: Gemini is the default, but `transcript.py` and `chat.py` securely support OpenAI, Anthropic, and Groq fallback models via environment variables.*

| Task / Provider | Model Used | Estimated Cost | Notes |
| :--- | :--- | :--- | :--- |
| **Gemini (Default)** | Gemini 2.5 Flash / Pro | **~$0.001 - $0.005** / meeting | Almost free on Google's current tier. Handles Summarization, Chat, and initial Multimodal tasks. |
| **OpenAI (Optional)** | GPT-4o / GPT-4o-mini | **~$0.02 - $0.15** / meeting | Configurable in UI. Higher cost, higher fidelity reasoning fallback. |
| **Anthropic (Optional)**| Claude 3.5 Sonnet | **~$0.03 - $0.10** / meeting | Configurable in UI. Excellent for complex summarization templates. |
| **Groq (Optional)** | Llama 3 (Text) | **~$0.005** / meeting | Extremely fast logic processing, very low token cost. |

### C. Web Search & Grounding (RAG)
*Code Audit: Grounded chat answers via RAG / RAG-Web.*

| Provider | Cost | Application |
| :--- | :--- | :--- |
| **Brave Search API** | **$3.00 / 1,000 queries** | General web scraping and fact-checking (`search_web` function). |
| **Gemini Grounding** | **~$35.00 / 1,000 queries**| Billed per grounded prompt if using native Google Grounding (confirm latest API pricing). |

---

## 3. Infrastructure Costs (Current vs. Proposed Migration)

The application currently relies on a traditional "Long-Running Worker" architecture (Compute Engine running Celery). We have a planned migration in `celery_to_gcp_migration.md` to shift to a "Serverless Trigger" model to cut idle costs.

### A. Current GCP Production Stack (Fixed Cost)
*Assumption: ~150 concurrent users. 24/7 Availability on traditional servers.*

| Component | Service | Spec | Cost / Month |
| :--- | :--- | :--- | :--- |
| **Backend / Celery Worker** | **GCP Compute Engine** | **e2-standard-2** (2 vCPU, 8GB RAM) | **$48.91** |
| **Frontend** | **GCP Cloud Run** | Auto-scaling container | **~$15.00** |
| **Database** | **Neon Postgres (Pro)** | 10GB Storage + Compute | **$19.00** |
| **Storage** | **GCS (Standard)** | 100GB Audio (Opus) | **$2.60** |
| **Disk** | **Persistent Disk** | 50GB SSD (Backend OS) | **$8.50** |
| **Total (Current)** | | | **~$94.00 / mo** |

### B. Proposed GCP Serverless Migration (Variable Cost)
*Goal: Replace Celery/Redis Compute instances with native, auto-scaling GCP services.*

| Current Component | New GCP Native Service | Estimated Cost / Month | Cost Benefit Analysis |
| :--- | :--- | :--- | :--- |
| **Broker/Store (Redis)** | **Cloud Memorystore** (Basic, 1GB) | **~$35.00** | Slightly higher base cost than self-hosting, but ZERO container maintenance/downtime risk. |
| **Task Queue (Celery)** | **Cloud Tasks** | **~$0.40 / million ops** | **Massive Savings.** Pay-per-use, completely eliminates the need for an always-on VM for background polling. |
| **Execution (Worker)** | **Cloud Run (Backend Worker)** | **~$15.00** (Variable) | Scale-to-zero capability. You only pay for CPU/RAM exactly when audio is uploading, summarizing, or diarizing. |
| **Total (Post-Migration)** | | **~$50.00 - $70.00 / mo** | Eliminates fixed Compute/Disk ($57). Memorystore ($35) becomes the main fixed cost. Overall potential savings of **20–40%**, with infinitely better burst scaling. |

---

## 4. Total Cost Per Meeting Scenarios

Evaluating total spend based on meeting type (1-hour length):

| Scenario | Service Stack Utilized | Estimated Cost |
| :--- | :--- | :--- |
| **Live Listening Only** | Groq (STT) + Gemini Flash (Summary) + Local Embeddings | **~$0.04** |
| **File Upload (Diarization)** | Deepgram (STT) + Gemini Flash (Summary) + Local Embeddings | **~$0.27** |
| **Audio-Enhanced Notes** | Gemini Multimodal API (`compressed audio + transcript`) | **+$0.001–$0.010** |
| **Active AI Participant** | Live STT ($0.04) + ElevenLabs TTS (Assuming 5 mins speech = $0.15) + Gemini Reasoning | **~$0.20** |
| **Premium API Config**| Deepgram Upload ($0.26) + OpenAI GPT-4o Summary ($0.10) | **~$0.36** |

---

## 5. Action Plan & Cost Monitoring

1. **Execute Serverless Migration:** Fast-track the `celery_to_gcp_migration.md` plan. Shifting the backend background tasks to **Cloud Tasks + Cloud Run** will drastically reduce idle compute waste and handle sudden influxes of large meeting uploads smoothly.
2. **Audio Compression:** Continue storing compressed archival audio (`recording.opus`) in GCP GCS as the primary artifact to keep the $2.60/100GB storage costs negligible.
3. **TTS Monitoring (ElevenLabs):** The AI Participant feature introduces the highest variable cost (ElevenLabs). Consider implementing a "budget limit" or switching to a cheaper TTS provider (e.g., Google TTS, OpenAI TTS) if the AI participant feature usage spikes.
4. **API Key Fallback Management:** With the recent update prioritizing `.env` keys over Database keys for Gemini, ensure production `.env` files are tightly managed using Google Secret Manager to prevent accidental quota bursts from compromised keys.

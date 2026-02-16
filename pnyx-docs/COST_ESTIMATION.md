# Cost Estimation: Meeting Co-Pilot (Production Audit)

**Date:** Feb 13, 2026
**Status:** Estimates based on **Audit of Actual Codebase** & Production Pricing.

---

## 1. Executive Summary

Our current architecture is **highly cost-optimized** by leveraging local embeddings and Google's aggressive pricing for Gemini models. The primary costs are **Infrastructure (GCP)** and **Transcription (Deepgram/Groq)**, not LLM tokens.

*   **Cost Per 1-Hour Meeting:** **~$0.04** (Standard) to **$0.30** (Premium Diarization), with **~$0.001–$0.005** additional optional spend for audio-enhanced notes generation.
*   **Monthly Infrastructure Cost:** **~$100** (Fixed GCP Compute + DB).
*   **Hidden Win:** Vector embeddings are running locally (Free), avoiding OpenAI embedding costs entirely.

---

## 2. Unit Economics (Per Hour of Audio)

### A. Speech-to-Text (STT) & Diarization
*Code Audit: Uses Groq (Whisper v3) for streaming, Deepgram (Nova-2) for uploads.*

| Service | Model | Cost / Hour | Role in Codebase |
| :--- | :--- | :--- | :--- |
| **Groq** | Whisper Large v3 | **$0.036** | **Live Streaming.** Insanely fast, cheap text. |
| **Deepgram** | Nova-2 | **$0.260** | **File Uploads.** Used for speaker diarization. |
| **Total (Hybrid)** | | **~$0.300** | If using "Double Pay" method (Uploads). |

> **Optimization:** Live meetings only cost $0.04/hr. Uploads cost ~$0.30/hr.

### B. LLM Intelligence (Summaries & Chat)
*Code Audit: Defaults to `gemini-2.5-flash`.*

| Task | Model Used | Cost / Meeting | Notes |
| :--- | :--- | :--- | :--- |
| **Summarization** | **Gemini 2.5 Flash** | **~$0.001** | effectively free on Google's tier. |
| **Chat / Q&A** | **Gemini 2.5 Flash** | **~$0.000** | Ultra-low cost per query. |
| **Alternative** | GPT-4o-mini | $0.002 | Supported fallback in code. |

### B2. Audio-Enhanced Notes (Gemini Multimodal)
*Assumption: Meeting notes generation can include transcript + meeting audio.*

| Mode | Extra Cost / 1-hour Meeting | Notes |
| :--- | :--- | :--- |
| **Transcript-only** | **Baseline** | Existing behavior, no extra audio inference path. |
| **Compressed audio + transcript (Recommended)** | **~$0.001–$0.005** | Best quality/latency tradeoff for notes generation. |
| **WAV + transcript** | **~$0.002–$0.010** | Similar quality, but higher transfer latency and higher processing overhead. |

> **Recommendation:** Keep `recording.wav` as archival source, but use a compressed derivative (for example `recording.notes.opus`) for model ingestion by default.

### C. Web Search (RAG)
*Assumption: Production Usage (Paid Tiers).*

| Provider | Cost | Notes |
| :--- | :--- | :--- |
| **Brave Search** | **$3.00–$5.00 / 1k** | **Recommended.** Pricing appears inconsistent between Brave's public page and dashboard; confirm actual rate in the Brave API dashboard before selecting. |
| **Gemini Grounding (Google Search)** | **~$35.00 / 1k grounded prompts** | **If using Gemini Grounding** for search. Billed per grounded prompt; confirm latest pricing before rollout. |
| **Tavily (Pro)** | $29.00 / mo | 2,500 searches. Good for complex agents. |

---

## 3. Feature-Wise Token Consumption (Audit)

We analyzed the codebase to calculate exact token loads per feature.
*Assumption: 1 hour audio = ~9,000 words transcript.*

### 1. Generating Summary (`summarization.py`)
*   **Logic:** Transcript is split into 5,000 char chunks (~1,250 tokens) with 1,000 char overlap.
*   **Chunks:** ~10 chunks for a 1-hour meeting.
*   **Per Chunk Cost:**
    *   Input: 1,250 (Text) + 350 (System Prompt) = **1,600 Tokens**
    *   Output: ~500 Tokens (JSON Schema)
*   **Total Per Meeting:** ~16,000 Input / ~5,000 Output.
*   **Cost (Gemini 2.5 Flash):** **<$0.002**

### 2. Chat / Q&A (`chat.py`)
*   **Logic:** RAG retrieves context + Web Search results + Chat History.
*   **Input Load:**
    *   Meeting Context: Up to 7,500 tokens (Truncated).
    *   RAG Results: ~2,500 tokens.
    *   History: ~1,000 tokens.
    *   **Total Input:** **~11,000 Tokens per query.**
*   **Output Load:** ~300 Tokens (Answer).
*   **Cost (Gemini 2.5 Flash):** **~$0.0001 per query.** (Negligible)

### 3. Web Research (`search_web` function)
*   **Logic:** Crawls 4 web pages, truncates each to 2,000 chars (~500 tokens).
*   **Input Load:** 4 pages * 500 tokens = **2,000 Tokens**.
*   **Output Load:** ~300 Tokens (Summary).
*   **Cost:** Negligible token cost. Main cost is the **Search API call ($0.003)**.

---

## 4. Infrastructure Costs (GCP Production Stack)
*Assumption: ~150 concurrent users. 24/7 Availability.*

| Component | Service | Spec | Cost / Month |
| :--- | :--- | :--- | :--- |
| **Backend** | **GCP Compute Engine** | **e2-standard-2** (2 vCPU, 8GB RAM) | **$48.91** |
| **Frontend** | **GCP Cloud Run** | Auto-scaling container | **~$15.00** |
| **Database** | **Neon Postgres (Pro)** | 10GB Storage + Compute | **$19.00** |
| **Storage** | **GCS (Standard)** | 100GB Audio (Opus) | **$2.60** |
| **Disk** | **Persistent Disk** | 50GB SSD (Backend OS) | **$8.50** |
| **Total** | | | **~$94.00 / mo** |

---

## 5. Total Cost Per Meeting Scenario

| Scenario | Stack | Cost |
| :--- | :--- | :--- |
| **Live Meeting (1hr)** | Groq + Gemini Flash + Local Embeddings | **$0.04** |
| **Uploaded File (1hr)** | Deepgram + Gemini Flash + Local Embeddings | **$0.27** |
| **Audio-Enhanced Notes Add-on (1hr)** | Gemini Flash multimodal (`compressed + transcript`) | **+$0.001–$0.005** |

---

## 6. Save-Time Optimization Impact (Option 1)

### 3-hour Meeting Finalization Time

| Flow | Save-Time Overhead |
| :--- | :--- |
| **Compress after Save (current heavy path)** | **~8 to 25 min** |
| **Parallel encode during recording (recommended)** | **~10 to 60 sec** |

**Expected save-time reduction:** ~95% to 99%.

---

## 7. Action Plan for Further Savings

1.  **Switch Upload Pipeline:** Currently, uploads might be using *both* Groq and Deepgram (Double Tax). Ensure `file_processing.py` uses Deepgram exclusively for uploads to save the $0.04 Groq fee per upload.
2.  **Audio Compression (Today Plan):** Keep PCM transcription pipeline unchanged, but run parallel encoding during recording and store compressed archival audio (`recording.opus`) in GCP as primary artifact.
3.  **Brave Search:** Implement Brave Search API integration to keep RAG costs low as search volume scales.
4.  **Pyannote (Future Optimization):** If Deepgram becomes too expensive at scale ($0.26/hr), implement a self-hosted Pyannote service (on a cheap GPU) + Groq. This would cut the upload cost from $0.27/hr to ~$0.14/hr (50% savings), though it adds operational complexity.

---

## 8. Security Addendum (Audio Encryption)

1. **Minimum for today:** Enable CMEK-backed encryption for GCP recording objects + strict KMS IAM.
2. **Preferred next hardening:** Add application-level envelope encryption for recordings with decrypt-on-arrival backend streaming.
3. **Access control:** Route sensitive audio retrieval through RBAC-protected endpoints rather than broad signed URL usage.


<!-- 
1). use of celery for better retries in upload audio
2). calender integration
3). share meeting to calender users
4). audio encryption
5). better prompts
6). user persona / journey 

-->

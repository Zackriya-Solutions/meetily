# Pnyx Analytics Implementation & Feature Tracking

## 1. Overview
As Pnyx evolves beyond its initial base into a highly specialized Meeting Co-Pilot, tracking custom feature usage becomes critical. This analytics implementation is designed specifically to measure engagement with Pnyx's core differentiators (e.g., Diarization, Catch Up, Ask AI, and Calendar/Mail integrations).

## 2. Expected Outcomes (Why We Are Doing This)

Implementing this feature-focused analytics pipeline will unlock several key product insights:

*   **Measure Feature ROI:** Understand whether advanced AI features (like "Refine Notes" and "Catch Up") are actually being used by users in the wild, justifying the LLM and development costs.
*   **Identify Friction Points:** Track where users drop off. If users generate notes but never share them, or if they import audio but cancel before diarization finishes, we can pinpoint UI/UX issues.
*   **Template & Content Strategy:** By tracking which "Generate Notes" templates are most popular, we can optimize default templates and create better presets for our users.
*   **Assess "Corporate Amnesia" Solutions:** Tracking how often "Cross-meeting context" is linked and how often notes are shared will prove whether Pnyx successfully solves the core problem of lost meeting context.
*   **Demonstrate Value Delivery:** Measuring exact counts of successful "AI Participant" interventions, shared notes, and exported recordings acts as our north star for product stickiness.

---

## 3. Features Covered & Events Tracked

We are adding targeted event tracking to the following custom Pnyx features:

### A. Core Ingestion & Audio 🎙️
| Feature | Tracked Event | Properties | Purpose |
| :--- | :--- | :--- | :--- |
| **Pnyx Bot (Mail/Calendar)** | `pnyx_bot_invited_via_mail` | `source`, `meeting_type` | How often are people inviting the bot vs recording manually? |
| **Calendar Context** | `calendar_context_fetched` | `status` | Does the bot successfully pull prior context before the meeting starts? |
| **Import Audio** | `audio_imported` | `duration`, `file_type` | Are users bringing past meetings into Pnyx? |
| **Diarization** | `diarization_requested` / `completed` | `speaker_count`, `duration` | Usage rate of speaker separation (a heavy compute feature). |

### B. In-Meeting Co-Pilot (Live) ⚡
| Feature | Tracked Event | Properties | Purpose |
| :--- | :--- | :--- | :--- |
| **Catch Up** | `catch_up_requested` | `time_range` (e.g. 5m, 15m) | Do people use the app to recover from zoning out? |
| **Ask AI** | `ask_ai_query` | `source` (live vs history) | Are users interacting with the transcript in real-time? |
| **AI Participant** | `ai_participant_interaction` | `trigger_type` | Is the AI actively contributing to the live discussion? |

### C. Post-Meeting Intelligence 🧠
| Feature | Tracked Event | Properties | Purpose |
| :--- | :--- | :--- | :--- |
| **Generate Notes** | `notes_generated` | `template_name`, `llm_model` | Are notes generated automatically vs manually? |
| **Template Switching** | `notes_template_switched` | `old_template`, `new_template` | Which meeting templates provide the most value? |
| **Refine Notes** | `notes_refined` | `prompt_length` | Do users rely on AI to polish notes before sharing? |
| **Cross-Meeting Context**| `cross_meeting_context_linked` | `linked_meeting_count` | Are users connecting the dots between past and present meetings? |

### D. Export & Value Delivery 📤
| Feature | Tracked Event | Properties | Purpose |
| :--- | :--- | :--- | :--- |
| **Share Notes** | `notes_shared` | `method` (email/link) | **The Ultimate Metric**: Is the output good enough to share with the team? |
| **Download Recording** | `recording_downloaded` | `format` | Are users extracting raw assets? |

---

## 4. Technical Architecture

The implementation will be broken down into three layers:

### Layer 1: The Database (SQLite / FastAPI)
*   **Table:** `analytics_events`
    *   `id` (UUID)
    *   `session_id` (String - maps to user session)
    *   `user_id` (String - persistent across sessions)
    *   `event_name` (String - e.g., "notes_shared")
    *   `properties` (JSON - contextual data like `template_name`)
    *   `timestamp` (DateTime)
*   **Endpoint:** `POST /api/analytics/track` to securely ingest events from the Next.js frontend.
*   **Endpoint:** `GET /api/analytics/dashboard/metrics` to aggregate data for the UI.

### Layer 2: Frontend Wiring (Next.js)
*   The existing `src/lib/analytics.ts` will be upgraded from local `console.log` stubs to execute background `fetch` requests to the FastAPI backend.
*   UI components (e.g., `RefineNotesSidebar.tsx`, `SummaryGeneratorButtonGroup.tsx`) will be instrumented with `Analytics.track()` calls tied to user actions (clicks, toggles, form submissions).

### Layer 3: The Pnyx Dashboard UI
*   A new admin/user dashboard located at `/dashboard` (accessible via the sidebar).
*   Built using `recharts` for clean, lightweight data visualization.
*   **Visualizations will include:**
    1.  *Hero KPIs:* Total Meetings Processed, Total Notes Shared, Bot Invites.
    2.  *Feature Radar:* A breakdown showing relative usage of Diarization vs Ask AI vs Catch Up.
    3.  *Template Leaderboard:* A pie chart showing the most heavily utilized summary templates.

# Architecture & Product Decision Record: AI Participant to Active AI Host

## 1. Executive Summary & Product Pivot

Following recent design reviews, the "AI Participant" feature is undergoing a significant product pivot. The original goal of building a passive "guardrail" system that warns users when they drift off-topic is being expanded. 

**Our new core goal is to elevate the AI into an Active Meeting Host.**
Instead of merely observing and reacting to bad meeting behavior, the AI will proactively lead, facilitate, and add value to the discussion in real-time.

Simultaneously, we are overhauling the User Experience (UX). A continuous, scrolling live transcript has proven to be highly distracting during live, in-person collaborative sessions. The UX will pivot away from a "transcript-first" design toward a clean, unobtrusive "widget-first" design focused on concise summaries and host interactions.

---

## 2. Product Vision: The Active AI Host

To move from a passive observer to an active host, the AI must take on the following responsibilities:

### A. Proactive Facilitation
*   **Meeting Kickoff:** The AI can optionally welcome participants, state the meeting goal, and outline the agenda based on calendar data.
*   **Time & Agenda Management:** Instead of just sending a "deviation" warning, the AI actively transitions topics: *"It sounds like we've settled on the marketing budget. We have 15 minutes left, should we move on to the Q3 roadmap?"*
*   **Conflict Resolution & Fact-Checking:** If a debate stalls, the AI can interject with relevant context from past meetings or its knowledge base to help resolve the blocker.
*   **Inclusivity & Engagement:** The AI can notice if someone hasn't spoken and prompt them: *"Sarah, we haven't heard your thoughts on the design changes yet."*

### B. "Less is More" UX (Widget & Side Panel)
*   **Hide the Transcript:** The full live text transcript will be hidden by default. Staring at rolling text pulls human attention away from other human participants.
*   **Subtle Side Panel / Extension:** The primary UI will be a small, unobtrusive widget (inspired by tools like Granola).
*   **High-Value Signals Only:** The UI will only surface:
    *   Current Agenda Item / Time Remaining.
    *   Concise, 1-2 line active summaries (e.g., "Decision: Marketing budget approved at $50k").
    *   Action items captured in real-time.

---

## 3. Architectural Pivot: Solving the Latency Bottleneck

The biggest barrier to building an *active host* is latency. An AI host cannot have a 3-5 second delay before speaking; it makes natural conversation impossible. 

The current pipeline (`Audio -> Whisper Transcription -> Text Buffer -> LLM Text Generation -> TTS Audio`) introduces too many round-trips and processing delays.

### A. Direct Audio-to-LLM Pipeline
We must bypass intermediate text generation entirely for the AI Host's cognitive reasoning.
*   **Multimodal Ingestion:** Feed raw audio chunks directly to an LLM capable of native audio understanding (e.g., Gemini 1.5 Pro/Flash native audio capabilities, or OpenAI Realtime API).
*   **Contextual Audio:** The LLM listens to the tone, interruptions, and pacing, which text transcripts completely lose.

### B. High-Fidelity, Ultra-Low Latency TTS
*   **ElevenLabs Integration:** To make the host sound natural and engaging, we will utilize the ElevenLabs API for text-to-speech, specifically targeting their lowest-latency streaming protocols or conversational AI agents.

---

## 4. Phased Implementation & Next Steps

To achieve this vision without breaking the existing solid foundation, we will execute the following technical phases:

### Phase 1: UX Redesign (The Summary Widget)
*   **Action:** Build the new `WidgetPanel` in the Next.js frontend.
*   **Action:** Hide the main live transcript view by default. Display a clean meeting dashboard that surfaces only the active agenda, captured decisions, and a rolling 2-line summary.

### Phase 2: Audio Pipeline R&D (Next Development Cycle)
*   **Action:** Modify `backend/app/services/ai_participant.py`. Currently, it uses a `RollingTranscriptBuffer`. We need to design a system that captures a rolling *audio buffer*.
*   **Action:** Prototype sending this raw audio directly to the Gemini API (or an equivalent native multimodal endpoint) to test latency and reasoning capability compared to the text-based guardrail system.
*   **Action:** Prototype an ElevenLabs TTS integration in the backend to stream audio responses back to the frontend.

### Phase 3: Active Host Behaviors (Upcoming Sprints)
*   **Action:** Expand the LLM's system prompt and capabilities from simply returning "Guardrail Alerts" to generating "Host Actions" (e.g., Agenda Transition, Direct Question, Summary Checkpoint).
*   **Action:** Implement "Host State Tracking" in the backend so the AI knows where it is in the agenda and what decisions have been firmly locked in.

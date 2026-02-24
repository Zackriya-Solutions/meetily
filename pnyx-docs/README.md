# Meeting Co-Pilot Documentation

**Meeting Co-Pilot** is a web-based collaborative meeting assistant built for real office use. This directory contains all documentation for building and deploying Meeting Co-Pilot.

> 📝 **Note**: This project is forked from Meetily (desktop app) but is being rebuilt as a web application with multi-user collaboration features for office teams.

---

## 📚 Documentation Index

### 1. Roadmap & Phases (`/roadmap`)
Development timeline, implementation plans, and status reports.

*   **[Phase 1](./roadmap/PHASE_1.md)** - Web Audio Foundation
*   **[Phase 3](./roadmap/PHASE_3.md)** - AI Features
*   **[Phase 4](./roadmap/PHASE_4.md)** - Cross-Meeting Context
*   **[Phase 5](./roadmap/PHASE_5.md)** - Post-Meeting & Polish
    *   **[Implementation Plan](./roadmap/PHASE_5_IMPLEMENTATION.md)** - Detailed Stabilization Plan
    *   **[Stabilization](./roadmap/STABILIZATION_PLAN.md)** - Stabilization Strategy
*   **[Phase 6](./roadmap/PHASE_6.md)** - Import Recording (Completed)
*   **[Phase 7](./roadmap/PHASE_7.md)** - Context-Aware Chatbot (In Progress)
*   **[Phase 8](./roadmap/PHASE_8.md)** - Polish & Production (Planned)
*   **[Phase 9](./roadmap/PHASE_9.md)** - Calendar Integration & Workflow Automation (Planned)
*   **[Future Optimizations](./roadmap/FUTURE_OPTIMIZATIONS.md)**

### 2. Architecture & Design (`/architecture`)
High-level system design, choices, and core concepts.

*   **[PRD](./architecture/PRD.md)** - Product Requirements Document
*   **[Context-Aware Chatbot](./architecture/CONTEXT_AWARE_CHATBOT_WITH_SEARCH.md)** - RAG & Search Architecture
*   **[Chat Memory](./architecture/CHAT_MEMORY_ARCHITECTURE.md)** - Complete storage and memory system
*   **[Context Flow](./architecture/CONTEXT_FLOW_EXPLAINED.md)** - How meeting context search works
*   **[Database Choice](./architecture/DATABASE_CHOICE.md)** - Why SQLite/Postgres/Neon?
*   **[Architecture Deviation Log](./architecture/ARCHITECTURE_DEVIATION_LOG.md)** - Record of changes from original plan

### 3. Feature Specifications (`/features`)
Detailed technical specifications for specific features.

*   **[Auth & RBAC](./features/AUTH_AND_RBAC.md)** - Authentication & Role-Based Access Control
*   **[RBAC Spec](./features/RBAC_SPEC.md)** - Permissions specification
*   **[Diarization](./features/DIARIZATION_PLAN.md)** - Speaker identification strategy
*   **[VAD Integration](./features/TEN_VAD_INTEGRATION_PLAN.md)** - Voice Activity Detection
*   **[Meeting Notes](./features/MEETING_NOTES_GENERATION.md)** - AI Note Generation
*   **[Notes Grounding Rework](./features/NOTES_GROUNDING_REWORK_PLAN.md)** - Simpler grounding + diarized regeneration strategy
*   **[Catch Me Up](./features/CATCH_ME_UP.md)** - Real-time summaries
*   **[Chat Interface](./features/CHAT_BASED_NOTE_INTERFACE.md)** - Dynamic AI refinement of notes
*   **[Cross-Meeting Search](./features/CROSS_MEETING_SEARCH.md)** - Vector search deep dive
*   **[Ask AI Context](./features/ASK_AI_CONTEXT.md)** - Linked vs Global context
*   **[API Keys](./features/USER_PROVIDED_API_KEYS.md)** - User-provided keys (BYOK)
*   **[Vector DB Disablement](./features/VECTOR_DB_DISABLEMENT.md)** - Notes on temporary disablement (Jan 2026)

---

## 🗂️ Repository Structure

```
meeting-co-pilot/
├── pnyx-docs/                   # Documentation Root
│   ├── roadmap/                 # Phased Implementation Plans
│   ├── architecture/            # Design & System Specs
│   └── features/                # Individual Feature Specs
│
├── CLAUDE.md                    # Main AI Agent Guide
├── backend/                     # FastAPI backend
└── frontend/                    # Next.js frontend
```

---

## 🎯 Project Goals

### What We're Building
A **web-based collaborative meeting assistant** for office teams:

1. **Real-Time Collaboration**
   - Multiple participants see same live transcript
   - No installation (just URL)
   - Works with room mics or laptops

2. **AI-Powered Features**
   - Live transcription (Whisper Large v3 via Groq)
   - Auto action item extraction
   - "Catch me up" for late joiners
   - Q&A with meeting context

3. **Cross-Meeting Intelligence**
   - Link related meetings
   - Surface past decisions
   - Track action items over time

---

**Version**: 0.2.0-beta
**Last Updated**: Jan 30, 2026

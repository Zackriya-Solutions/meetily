# Meeting Notes Generation

Automatic meeting notes generation using Gemini AI with selectable templates for different meeting types.

## Overview

When a meeting ends and has transcripts, notes are automatically generated using the selected template. Users can also manually trigger generation or regenerate with a different template.

## Features

- **Auto-Generation**: Notes generate automatically when meeting details page loads with transcripts
- **Template Selection**: Choose from 5 meeting templates before/after generation
- **Auto-Regeneration**: Changing template automatically regenerates notes
- **Copy to Clipboard**: Copy generated notes with one click
- **Background Processing**: UI remains responsive during generation

## Available Templates

| Template | ID | Use Case |
|----------|-----|----------|
| Standard Meeting | `standard_meeting` | General meetings with action items, decisions, next steps |
| Daily Standup | `daily_standup` | Team standups with progress, plans, blockers |
| Interview | `interview` | Candidate assessment, strengths, concerns |
| Brainstorming | `brainstorming` | Ideas, themes, creative concepts |
| Stand Up | `standup` | Quick updates, blockers, and action items |

## API Endpoints

### Generate Notes

**Endpoint:** `POST /meetings/{meeting_id}/generate-notes`

**Request Body (optional):**
```json
{
  "template_id": "standard_meeting",
  "model": "gemini",
  "model_name": "gemini-2.5-flash",
  "use_audio_context": true,
  "audio_mode": "auto",
  "audio_url": "",
  "max_audio_minutes": 120
}
```

`audio_mode` options: `auto | compressed | wav | transcript_only`

**Response:**
```json
{
  "message": "Notes generation started",
  "meeting_id": "meeting-123",
  "template_id": "standard_meeting",
  "status": "processing"
}
```

### Check Notes Status

**Endpoint:** `GET /get-summary/{meeting_id}`

Returns the generated notes when `status` is `"completed"`.

## Frontend Flow

```
1. User opens Meeting Details page
       ↓
2. Check if transcripts exist but no summary
       ↓
3. Auto-trigger notes generation with selected template
       ↓
4. Show loading spinner during generation
       ↓
5. Display generated notes when ready
       ↓
6. User changes template → Auto-regenerate notes
```

## Implementation Files

### Backend
- `backend/app/main.py` - API endpoints and template prompts
- `get_template_prompt()` - Template definitions
- `generate_notes_with_gemini_background()` - Async generation

### Frontend
- `frontend/src/hooks/meeting-details/useTemplates.ts` - Template state management
- `frontend/src/hooks/meeting-details/useSummaryGeneration.ts` - Generation logic
- `frontend/src/app/meeting-details/page-content.tsx` - Auto-generation triggers
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx` - Notes display

## Configuration

### Gemini API Key

Set one of these environment variables:
- `GEMINI_API_KEY`

Or configure via Settings → AI Model in the UI.

## Diagram

```
┌─────────────────────────────────────────────────────────────┐
│   Meeting Details Page                                      │
│  ┌───────────────────┐  ┌────────────────────────────────┐ │
│  │   Transcripts     │  │  Notes Panel                   │ │
│  │   • Segment 1     │  │  [Template Dropdown ▼]         │ │
│  │   • Segment 2     │  │  ┌──────────────────────────┐  │ │
│  │   • Segment 3     │  │  │  Generated Notes         │  │ │
│  │                   │  │  │  • Key Points           │  │ │
│  │                   │  │  │  • Decisions            │  │ │
│  │                   │  │  │  • Action Items         │  │ │
│  │                   │  │  └──────────────────────────┘  │ │
│  │                   │  │  [Copy] [Regenerate]           │ │
│  └───────────────────┘  └────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                           ↑
           Template Change triggers regeneration
```

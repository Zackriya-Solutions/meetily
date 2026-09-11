# Meeting Summary Templates

This directory contains template definitions for meeting summary generation.

## Available Templates

### 1. `daily_standup.json`
Time-boxed daily updates template designed for engineering/product teams.

**Sections:**
- Date
- Attendees
- Yesterday (completed work)
- Today (planned work)
- Blockers
- Notes

### 2. `standard_meeting.json`
General-purpose meeting notes template focusing on key outcomes and actions.

**Sections:**
- Summary
- Key Decisions
- Action Items
- Discussion Highlights

## Template Structure

Each template JSON file follows this schema:

```json
{
  "name": "Template Name",
  "description": "Brief description of the template's purpose",
  "sections": [
    {
      "title": "Section Title",
      "instruction": "Instructions for the LLM on what to extract/include",
      "format": "paragraph|list|string",
      "item_format": "Optional: Markdown table format for list items"
    }
  ]
}
```

## Custom Templates

Users can add custom templates to the application data directory:

- **macOS**: `~/Library/Application Support/Meetily/templates/`
- **Windows**: `%APPDATA%\Meetily\templates\`
- **Linux**: `~/.config/Meetily/templates/`

Custom templates override built-in templates with the same filename.

## Template Fields

### Root Level
- `name` (required): Display name for the template
- `description` (required): Brief explanation of the template's use case
- `sections` (required): Array of section definitions

### Section Object
- `title` (required): Section heading text
- `instruction` (required): LLM guidance for this section
- `format` (required): One of `"paragraph"`, `"list"`, or `"string"`
- `item_format` (optional): Markdown formatting hint for list items (e.g., table structure)
- `example_item_format` (optional): Alternative formatting hint

## Usage in Code

Templates are loaded using the `templates` module:

```rust
use crate::summary::templates;

// Get a specific template
let template = templates::get_template("daily_standup")?;

// List available templates
let available = templates::list_templates();

// Validate custom template JSON
let custom_json = std::fs::read_to_string("custom.json")?;
let validated = templates::validate_template(&custom_json)?;
```

## Dates in summaries

Summary generation supplies a separate `meeting_metadata` block containing
`record_created_at_utc`, the saved meeting record timestamp in RFC 3339 UTC form
(for example, `2026-01-01T09:00:00Z`). It uses the stored timestamp when
regenerating an older meeting, not the current clock. This metadata reaches the
final report even when a long transcript is summarized in chunks.

A custom Date section can use an instruction such as:

```json
{
  "title": "Date",
  "instruction": "Use an explicitly stated meeting date from the transcript or user context. Otherwise show record_created_at_utc from meeting_metadata, labeled Saved record date (UTC). If neither is available, state that the date was not provided.",
  "format": "paragraph"
}
```

For imported recordings, the saved timestamp may be the **import time**, not when
the conversation happened. Include the actual date in user context when known.
The timestamp has an explicit UTC timezone; it is not converted to the viewer's
local time. It must not be used to guess deadlines or resolve relative dates such
as “tomorrow.” Existing templates need no new placeholders and retain their
section structure. Templates without a Date section do not require one.

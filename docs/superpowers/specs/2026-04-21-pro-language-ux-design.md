# Design: Pro-style Summary Language UX (FR-1 extension)

**Date:** 2026-04-21
**Branch:** feat/multi-language-summary
**Upstream target:** Zackriya-Solutions/meetily devtest (issue #413)
**Status:** Approved — ready for implementation plan

---

## Problem

FR-1 landed a global default summary language (a plain dropdown in Settings). The Meetily Pro build ships two richer patterns that belong in OSS:

- **Pattern A:** chip-based recents list in Settings, replacing the dropdown
- **Pattern B:** in-meeting language picker popover adjacent to Re-generate Summary, with per-meeting override that persists

---

## Scope

**In scope (single PR):**
- Replace `SummaryLanguageSettings` dropdown with chip editor (max 5, auto-MRU)
- New `LanguagePickerPopover` component in the meeting detail view
- New `useRecentLanguages` hook (localStorage, no DB table)
- New migration: `meetings.summaryLanguage TEXT`
- Two new Tauri commands: `api_get_meeting_summary_language`, `api_set_meeting_summary_language`
- Extend `api_process_transcript` with `summary_language: Option<String>` param
- Resolution cascade in `service.rs`

**Out of scope:**
- Regional BCP-47 variants (`en-GB`, `en-US`) — bare ISO-639-1 codes only
- Frecency weighting — simple MRU (push-front, dedupe, trim to 5)
- Per-chunk language detection or mid-summary language switching
- Transcript language auto-detection (Whisper `auto` → summary defers to model)

---

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | C — both patterns in one PR | Full Pro parity; single reviewable diff |
| Per-meeting persistence | Persistent on `meetings` row | Meetings are reopened days later; ephemerality feels like a bug |
| Auto semantics | Match transcription language setting | Aligns with DeepL/Google Translate/Meet convention |
| Regional variants | Bare ISO codes only | Upstream Pro uses bare codes; consumer apps don't split regionals |
| Recents storage | localStorage, max 5, MRU | Follows `ConfigContext.primaryLanguage` precedent; no new DB table |
| Multi-language transcription | Honor transcription *setting*, not detection | Simple, deterministic; caveat acceptable |
| Implementation approach | Single PR + `useRecentLanguages` hook | Atomic, reviewable; hook avoids new React context overhead |

---

## Architecture

Four layers touched. Green = new, Blue = modified.

### Frontend (React / TypeScript)

| File | Change |
|---|---|
| `src/hooks/useRecentLanguages.ts` | **NEW** — MRU list in localStorage |
| `src/components/LanguagePickerPopover.tsx` | **NEW** — search + recents + all-languages popover |
| `src/components/SummaryLanguageSettings.tsx` | **MODIFIED** — replace `<select>` with chip editor |
| `src/components/MeetingDetails/SummaryPanel.tsx` | **MODIFIED** — mount popover + language button |
| `src/hooks/meeting-details/useSummaryGeneration.ts` | **MODIFIED** — add `summaryLanguage` param |

### Tauri Commands (Rust)

| Command | Change |
|---|---|
| `api_get_meeting_summary_language` | **NEW** |
| `api_set_meeting_summary_language` | **NEW** |
| `api_process_transcript` | **MODIFIED** — add `summary_language: Option<String>` |
| `api_get_summary_language` | unchanged |
| `api_set_summary_language` | unchanged |

### Service / Processor (Rust)

| File | Change |
|---|---|
| `summary/service.rs` | **MODIFIED** — resolution cascade |
| `summary/processor.rs` | unchanged — `language_directive()` already handles None |

### Database

| File | Change |
|---|---|
| `migrations/20260420_add_meeting_summary_language.sql` | **NEW** |
| `database/models.rs` — `MeetingModel` | **MODIFIED** — add `summary_language` field |
| `database/repositories/meeting.rs` | **MODIFIED** — two new methods |

---

## Data Model

### Migration

```sql
-- migrations/20260420_add_meeting_summary_language.sql
ALTER TABLE meetings ADD COLUMN summaryLanguage TEXT;
```

### MeetingModel (Rust)

```rust
#[sqlx(rename = "summaryLanguage")]
#[serde(rename = "summaryLanguage")]
pub summary_language: Option<String>,
```

### MeetingsRepository (Rust)

```rust
pub async fn get_meeting_summary_language(pool, meeting_id) -> Result<Option<String>, sqlx::Error>
pub async fn set_meeting_summary_language(pool, meeting_id, language: Option<&str>) -> Result<(), sqlx::Error>
```

### useRecentLanguages (TypeScript)

```ts
const MRU_KEY = 'summaryLanguageRecents';
const MAX_RECENTS = 5;

// Returns: { recents: string[], addRecent: (code: string) => void }
// addRecent: push to front, dedupe by code, trim to MAX_RECENTS, persist
```

---

## Language Resolution (service.rs)

```
summary_language =
  meetings.summaryLanguage        // per-meeting override (highest priority)
  ?? settings.summaryLanguage     // global default
  ?? transcription_lang_if_known  // Auto: transcription setting if not 'auto'/'auto-translate'
  ?? None                         // no directive injected (model decides)
```

Transcription language is read from the same setting that `ConfigContext.selectedLanguage` persists (`primaryLanguage` in localStorage / `LANGUAGE_PREFERENCE` in Rust). If it is `auto` or `auto-translate`, this step yields `None`.

---

## UI Components

### Pattern A — SummaryLanguageSettings (Settings card)

- Header: globe icon + "Summary Language"
- Body: chip row (ISO code label + × button per chip) + "+ Add language" button
- Footer hint: "Quick-switch options in the summary generator (max 5)"
- "+ Add language" opens `LanguagePickerPopover` in settings mode (no recents section shown, all-languages only)
- Removing a chip calls `api_set_summary_language(null)` if it was the current global default, else just removes from MRU

### Pattern B — LanguagePickerPopover (Meeting detail)

- Trigger: "🌐 Auto ▾" button adjacent to "Re-generate Summary" in `SummaryPanel`
- Popover sections:
  1. Always-on search input (filters both sections)
  2. "Recently Used" — codes from `useRecentLanguages`, hidden if empty
  3. Divider
  4. "All Languages" — full list, "Auto" pinned first with checkmark when active
- Selecting a language: calls `api_set_meeting_summary_language`, calls `addRecent(code)`, updates button label
- Selecting Auto: calls `api_set_meeting_summary_language(null)`
- Button label: shows selected language name ("Russian") or "Auto" when unset

---

## Regenerate Flow

```
User opens popover → selects "Russian"
  → api_set_meeting_summary_language(meetingId, 'ru')   [immediate, no regenerate]
  → addRecent('ru')                                       [updates localStorage]
  → button label updates to "Russian"

User clicks "Re-generate Summary"
  → useSummaryGeneration.processSummary({ ..., summaryLanguage: 'ru' })
  → invoke('api_process_transcript', { ..., summaryLanguage: 'ru' })
  → service.rs resolution: per-meeting 'ru' wins
  → language_directive() injects "Produce the entire response in Russian"
```

Note: `summaryLanguage` param on `api_process_transcript` is advisory — the service re-reads from DB as source of truth. The param is passed for logging/tracing only.

---

## Error Handling

| Scenario | Handling |
|---|---|
| Picker save fails | Revert to previous selection + `toast.error` |
| Old DB (column absent) | SQLx returns `None`, resolver falls to next tier |
| Unknown language code | `language_directive()` returns `""` — no-op |
| localStorage read fails | Default to `[]`, silent (cosmetic state only) |
| `api_process_transcript` param missing | Rust `Option<String>` — defaults to `None`, existing behaviour |

---

## Out-of-scope Follow-ups

- Mid-meeting language detection prompt ("We detected Russian in transcript — switch?")
- Frecency weighting on recents
- Per-template language override
- Regional variant support (`en-GB`, `en-US`)

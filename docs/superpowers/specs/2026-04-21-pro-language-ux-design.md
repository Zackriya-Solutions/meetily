# Design: Pro-style Summary Language UX (FR-1 extension)

**Date:** 2026-04-21
**Branch:** feat/multi-language-summary
**Upstream target:** Zackriya-Solutions/meetily devtest (issue #413)
**Status:** Approved (rev 3) — ready for implementation plan

**Rev 3 changes:** ALL summary language state (global default, per-meeting override, recents) moves to localStorage. The FR-1 MVP migration SQL and its associated Rust plumbing (Setting struct field, SettingsRepository methods, `api_get/set_summary_language` commands) are removed from the branch. Backend becomes nearly stateless w.r.t. summary language — it only accepts a `summary_language: Option<String>` param on `api_process_transcript`. A local DB cleanup snippet is provided for any developer who previously ran the MVP migration.

---

## Problem

FR-1 was landed on this branch with a DB migration approach: a `settings.summaryLanguage` column + a pair of Tauri commands + a native `<select>` dropdown. That approach has two weaknesses for an OSS PR:

1. **Migration drift risk** — any tweak to the migration file after first apply breaks dev environments (observed: checksum mismatch panic at startup on this very branch).
2. **Mismatched complexity** — a single-device, local-first app with ephemeral UI preferences does not need a DB round-trip for each read/write. localStorage is the right granularity.

Additionally: when the user picks a non-English summary language, the built-in template's section headings (written in English in the template prompt) can remain in English — a mixed-language summary.

---

## Scope

**In scope (single PR):**

Backend revert (baseline cleanup):
- Remove migration `20260419000000_add_summary_language.sql`
- Revert `Setting` struct (drop `summary_language` field)
- Revert `SettingsRepository` (drop `get_summary_language` / `save_summary_language`)
- Revert `api_get_summary_language` / `api_set_summary_language` Tauri commands + their `mod.rs` re-exports + their `lib.rs` registrations

Backend additions:
- `api_process_transcript` gains `summary_language: Option<String>` param (load-bearing)
- `service.rs` cascade: param -> transcription lang if known -> None (no DB tier — all local)
- `processor.rs` `language_name_from_code` dropped regional variants (bare ISO only) [already on branch]
- `processor.rs` `language_directive` strengthened to cover section headings (Option A)

Frontend additions:
- `src/lib/summary-languages.ts` — shared `LANGUAGE_OPTIONS`, `labelForCode`, `AUTO_VALUE`
- `src/hooks/useRecentLanguages.ts` — MRU list in localStorage
- `src/components/LanguagePickerPopover.tsx` — search + recents + all-languages popover (controlled)
- `src/components/SummaryLanguageSettings.tsx` — chip editor (localStorage-backed; no Tauri invokes)
- `src/components/MeetingDetails/SummaryPanel.tsx` — language button + popover; R/W of `summaryLanguage:<meetingId>`
- `src/hooks/meeting-details/useSummaryGeneration.ts` — resolves language client-side, forwards to Tauri

Local cleanup (not shipped):
- SQL snippet for any dev who previously applied the MVP migration; documented in the PR description

**Out of scope:**
- Any DB migration for summary language
- Regional BCP-47 variants (`en-GB`, `en-US`) — bare ISO-639-1 only
- Frecency weighting on recents
- Per-language translated template headings (Option B / C — follow-up if Option A is insufficient)
- Per-chunk language detection or mid-summary switching
- Transcript language auto-detection (Whisper `auto` -> summary defers to model)

---

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Full Pro parity (chips + popover) in one PR | Atomic, reviewable |
| Global default storage | **localStorage** | Avoids DB migration entirely; local-first single-device app |
| Per-meeting override storage | **localStorage**, keyed by meeting_id | Same reason; no cross-device sync requirement |
| Recents storage | **localStorage**, max 5, MRU | Follows `ConfigContext.primaryLanguage` precedent |
| Auto semantics | Match transcription language setting | DeepL / Google Translate / Meet convention |
| Regional variants | Bare ISO codes only | Upstream Pro convention; small models don't reliably honour regional tags |
| Multi-language transcription | Honor transcription *setting*, not detection | Simple, deterministic; caveat acceptable |
| Template heading translation | Option A — strengthen prompt directive | One-line prompt change; works for built-in AND custom templates |
| Resolution ownership | Frontend (JavaScript) | Backend stays stateless w.r.t. summary language; easier to test client-side |
| MVP migration | **Removed from branch** | Every dev who ran the MVP runs a one-off cleanup locally; no migration complexity shipped |

---

## Architecture

Two layers touched. Green = new, Blue = modified, Red = reverted.

### Frontend (React / TypeScript)

| File | Change |
|---|---|
| `src/lib/summary-languages.ts` | **NEW** — shared `LANGUAGE_OPTIONS`, `labelForCode`, `AUTO_VALUE` |
| `src/hooks/useRecentLanguages.ts` | **NEW** — MRU list in localStorage |
| `src/components/LanguagePickerPopover.tsx` | **NEW** — search + recents + all-languages popover |
| `src/components/SummaryLanguageSettings.tsx` | **NEW / REWRITE** — chip editor |
| `src/components/SummaryModelSettings.tsx` | **MODIFIED** — mounts `<SummaryLanguageSettings />` [already on branch] |
| `src/components/MeetingDetails/SummaryPanel.tsx` | **MODIFIED** — language button + popover mount |
| `src/hooks/meeting-details/useSummaryGeneration.ts` | **MODIFIED** — resolve lang via localStorage, forward as `summaryLanguage` param |

### Backend (Rust)

| File | Change |
|---|---|
| `migrations/20260419000000_add_summary_language.sql` | **REVERTED / DELETED** — file removed from branch |
| `database/models.rs` | **REVERTED** — `Setting` struct `summary_language` field dropped |
| `database/repositories/setting.rs` | **REVERTED** — `get/save_summary_language` methods dropped |
| `summary/commands.rs` | **REVERTED + MODIFIED** — drop `api_get/set_summary_language`; add `summary_language: Option<String>` param to `api_process_transcript` |
| `summary/mod.rs` | **REVERTED** — drop re-exports for the two removed commands |
| `lib.rs` | **REVERTED** — drop registrations for the two removed commands |
| `summary/service.rs` | **MODIFIED** — accept language param via background-task signature, use in cascade |
| `summary/processor.rs` | **MODIFIED** — strengthen `language_directive`; keep `language_name_from_code` (regional variants already dropped) |

---

## Data Model — all localStorage

```
Key: summaryLanguageDefault
Value: ISO-639-1 code (e.g. "en") or absent = Auto
Purpose: global default applied when a meeting has no per-meeting override

Key: summaryLanguage:<meetingId>
Value: ISO-639-1 code or absent = fall through to default/auto
Purpose: per-meeting override

Key: summaryLanguageRecents
Value: JSON array of up to 5 ISO-639-1 codes, MRU-first
Purpose: quick-switch list shown in the popover's "Recently Used" section
```

Absent keys and empty strings both mean "no value set at this tier".

---

## Language Resolution (frontend — `useSummaryGeneration.resolveSummaryLanguage`)

```ts
function resolveSummaryLanguage(meetingId: string): string | null {
  // 1. Per-meeting override
  const perMeeting = localStorage.getItem(`summaryLanguage:${meetingId}`);
  if (perMeeting) return perMeeting;

  // 2. Global default
  const defaultLang = localStorage.getItem('summaryLanguageDefault');
  if (defaultLang) return defaultLang;

  // 3. Auto: match transcription language if it's a specific language
  const transcription = localStorage.getItem('primaryLanguage');
  if (transcription && transcription !== 'auto' && transcription !== 'auto-translate') {
    return transcription;
  }

  // 4. No override — backend goes "model decides"
  return null;
}
```

The resolved value flows into `invoke('api_process_transcript', { summaryLanguage, ... })`. Backend:

```rust
// service.rs
// 1. Use the param directly if non-null / non-empty (already trimmed by command layer)
// 2. Else None — no directive injected, model decides from transcript content
```

No backend cascade — resolution is one-tier on the Rust side.

---

## Template Heading Handling (Option A)

`language_directive` in `processor.rs` is strengthened from:

> "Produce the entire response in {Name}. Do not translate proper nouns, code, or quoted material."

to:

> "Produce the entire response in {Name}, **including section headings, labels, and list markers**. Do not translate proper nouns, code, file paths, or quoted material."

Rationale: built-in templates contain English literals like `## Action Items` embedded in the prompt as structural instructions. Small models (Qwen 3.5 2B, Gemma 3 1B) often preserve these verbatim, producing mixed-language output. Explicitly instructing the model to translate structural labels addresses this without touching template files. Also covers user-created custom templates.

---

## UI Components

### Pattern A — SummaryLanguageSettings (Settings card)

- Header: globe icon + "Summary Language"
- Body: chip row (label + × button per chip, from `useRecentLanguages`) + "+ Add language" button
- Footer hint: "Quick-switch options in the summary generator (max 5)"
- "+ Add language" opens `LanguagePickerPopover` in settings mode (no Auto option, no Recently Used section)
- Removing a chip removes it from the MRU list only; no side-effect on the global default

### Pattern B — LanguagePickerPopover (Meeting detail)

- Trigger: "🌐 {label} ▾" button adjacent to Re-generate; label shows selected language or "Auto"
- Popover sections:
  1. Always-on search input (filters both)
  2. "Recently Used" — codes from `useRecentLanguages`, hidden if empty
  3. Divider
  4. "All Languages" — full list; Auto pinned first with ✓ when active
- Controlled component: `value` + `onChange` from caller. Does not read or write localStorage itself; the caller (SummaryPanel or SummaryLanguageSettings) owns persistence.

---

## Regenerate Flow

```
User opens popover and selects "Russian"
  SummaryPanel.onLangChange('ru'):
    localStorage.setItem('summaryLanguage:meeting-abc', 'ru')
    addRecent('ru')                                  // updates MRU
    setSummaryLang('ru')                             // button label updates

User clicks "Re-generate Summary"
  useSummaryGeneration.handleRegenerateSummary()
    const lang = resolveSummaryLanguage(meetingId)   // reads all 3 localStorage keys in order
    processSummary({ transcriptText, summaryLanguage: lang, isRegeneration: true })
  invoke('api_process_transcript', { summaryLanguage: 'ru', ... })
  service.rs receives 'ru', passes to generate_meeting_summary
  language_directive injects "Produce the entire response in Russian, including section headings..."
```

---

## Error Handling

| Scenario | Handling |
|---|---|
| localStorage read fails (incognito / quota) | Treat key as absent; silent, falls through cascade |
| localStorage write fails | Silent in hook; local state still updates for immediate UI feedback |
| `summary_language` param empty string | Rust trims and normalises to None; no directive injected |
| Unknown language code | `language_directive` returns `""` — no-op; model generates without directive |
| Missing migration file but DB has record | Dev cleanup snippet below; not a concern for fresh users |

---

## Local DB Cleanup (not shipped — dev-only)

Any developer who previously ran the FR-1 MVP (which included migration `20260419000000`) needs to clean up their local DB after pulling this branch, because sqlx will panic on startup when it finds a DB record for a migration file that no longer exists on disk.

**Cleanup SQL (run once per dev who ran the MVP):**

```sql
BEGIN;
DELETE FROM _sqlx_migrations WHERE version = 20260419000000;
ALTER TABLE settings DROP COLUMN summaryLanguage;
COMMIT;
```

**Path:** `~/Library/Application Support/com.meetily.ai/meeting_minutes.sqlite` (macOS)
**Command:**
```bash
sqlite3 ~/Library/Application\ Support/com.meetily.ai/meeting_minutes.sqlite \
  "BEGIN; DELETE FROM _sqlx_migrations WHERE version = 20260419000000; ALTER TABLE settings DROP COLUMN summaryLanguage; COMMIT;"
```

This snippet goes in the PR description, not the codebase. Fresh users (who never ran the MVP migration) don't need it — their DB was never touched.

---

## Out-of-scope Follow-ups

- Option B: per-language translated template headings (in-prompt heading substitution) if Option A proves insufficient
- DB-backed per-meeting override (if cross-device sync is ever added)
- Mid-meeting language detection prompt ("We detected Russian in transcript — switch?")
- Frecency weighting on recents
- Per-template language override
- Regional variant support (`en-GB`, `en-US`)

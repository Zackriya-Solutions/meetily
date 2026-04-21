# Pro-style Summary Language UX Implementation Plan (Rev 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land Pro-style summary language UX (chip editor in Settings + in-meeting popover with per-meeting override) on top of a simplified FR-1 backend where ALL language state is localStorage-driven and the DB migration is pulled out of the branch.

**Architecture:** Frontend-owned state. Backend stays almost stateless w.r.t. summary language — `api_process_transcript` gains a `summary_language: Option<String>` param and `service.rs` forwards it to the prompt builder. The FR-1 MVP migration + associated Rust plumbing is reverted as a prerequisite.

**Tech Stack:** Rust (Tauri, SQLx, Tokio), TypeScript (React, Next.js), Tailwind CSS.

**Spec:** `docs/superpowers/specs/2026-04-21-pro-language-ux-design.md` (rev 3)

**Prerequisite for any dev who applied the MVP migration locally:**
```bash
sqlite3 ~/Library/Application\ Support/com.meetily.ai/meeting_minutes.sqlite \
  "BEGIN; DELETE FROM _sqlx_migrations WHERE version = 20260419000000; ALTER TABLE settings DROP COLUMN summaryLanguage; COMMIT;"
```
This is a one-off developer action. It is NOT part of the commit history.

---

## File Structure

| Path | Status | Purpose |
|---|---|---|
| `frontend/src-tauri/migrations/20260419000000_add_summary_language.sql` | **DELETE** | Remove the FR-1 MVP migration |
| `frontend/src-tauri/src/database/models.rs` | REVERT | Drop `Setting.summary_language` field |
| `frontend/src-tauri/src/database/repositories/setting.rs` | REVERT | Drop `get_summary_language` / `save_summary_language` |
| `frontend/src-tauri/src/summary/commands.rs` | REVERT + MODIFY | Drop `api_get/set_summary_language`; add `summary_language` param on `api_process_transcript` |
| `frontend/src-tauri/src/summary/mod.rs` | REVERT | Drop removed-command re-exports |
| `frontend/src-tauri/src/lib.rs` | REVERT | Drop removed-command registrations |
| `frontend/src-tauri/src/summary/service.rs` | MODIFY | Receive language via function param, pass to generator |
| `frontend/src-tauri/src/summary/processor.rs` | MODIFY | Strengthen `language_directive` for Option A |
| `frontend/src/lib/summary-languages.ts` | NEW | Shared language options + helpers |
| `frontend/src/hooks/useRecentLanguages.ts` | NEW | MRU hook (localStorage) |
| `frontend/src/components/LanguagePickerPopover.tsx` | NEW | Controlled search + recents + all popover |
| `frontend/src/components/SummaryLanguageSettings.tsx` | REWRITE | Chip editor, localStorage only |
| `frontend/src/components/MeetingDetails/SummaryPanel.tsx` | MODIFY | Language button + popover mount |
| `frontend/src/hooks/meeting-details/useSummaryGeneration.ts` | MODIFY | Resolve lang, forward as `summaryLanguage` param |

---

## Task 1: Backend revert — FR-1 MVP backend rolled back

Goal: restore the Rust files touched by FR-1 MVP to their `main`-branch state, delete the migration file. This establishes a clean baseline for the Pro UX additions.

**Files:**
- Delete: `frontend/src-tauri/migrations/20260419000000_add_summary_language.sql`
- Revert to `main`: `database/models.rs`, `database/repositories/setting.rs`, `summary/commands.rs`, `summary/mod.rs`, `lib.rs`

Note: `summary/processor.rs` and `summary/service.rs` are also modified on this branch, but they stay modified — we keep the `language_directive`, `language_name_from_code`, and the function-parameter-based language passing. They will be further modified in Tasks 3-4.

- [ ] **Step 1: Delete the migration file**

```bash
rm frontend/src-tauri/migrations/20260419000000_add_summary_language.sql
```

- [ ] **Step 2: Revert the fully-revertable files to main**

```bash
git checkout main -- \
  frontend/src-tauri/src/database/models.rs \
  frontend/src-tauri/src/database/repositories/setting.rs \
  frontend/src-tauri/src/summary/mod.rs
```

This wipes the `Setting.summary_language` field, the two repository methods, and the `mod.rs` re-exports of the two removed commands.

- [ ] **Step 3: Surgically revert `summary/commands.rs`**

Do NOT `git checkout main` on this file — we want to keep `summary/commands.rs` as-is minus the two removed commands and the imports that depended on them.

Remove the full bodies of `api_get_summary_language` and `api_set_summary_language` (they sit between the `api_process_transcript` function and `api_cancel_summary` function — around lines 242-276 of the current branch version).

If `use crate::database::repositories::setting::SettingsRepository` is in the file, leave it — it is also used by the module's parent code path (not removed). Actually, check: search for all uses of `SettingsRepository` in `commands.rs` and remove the import only if no other uses remain.

- [ ] **Step 4: Surgically revert `lib.rs`**

Remove the two lines registering the removed commands from the `invoke_handler!` macro:
```rust
summary::api_get_summary_language,
summary::api_set_summary_language,
```

Leave all other `summary::...` registrations in place.

- [ ] **Step 5: Verify Rust compiles**

```bash
cd frontend/src-tauri && cargo check --features metal
```

Expected: compiles with pre-existing warnings only. If errors reference missing `SettingsRepository::get_summary_language` etc. — those are from `summary/service.rs` which still references them. That is expected; Task 3 will clean this up. For now, it's OK to defer if the only errors are in `service.rs` paths that Task 3 will rewrite — the user can choose to fix this within Task 1 by commenting out the summary_language fetch in service.rs temporarily, or roll Task 3 into this task's commit.

Recommended: bundle Task 3 (service.rs cascade rewrite) into this task's commit so cargo check passes cleanly end-to-end. See Task 3 below for the replacement block — apply it now instead of waiting.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
refactor: revert FR-1 MVP DB migration for summary language

Pivots the summary-language feature to be fully localStorage-driven.
Removes the migration file, the Setting.summary_language column and
its repository methods, and the api_get/set_summary_language Tauri
commands. Service.rs will accept the language via function parameter
(wired in a follow-up commit).

Any developer who previously ran the MVP migration locally must run:

  sqlite3 ~/Library/Application\ Support/com.meetily.ai/meeting_minutes.sqlite \\
    "BEGIN; DELETE FROM _sqlx_migrations WHERE version = 20260419000000; \\
     ALTER TABLE settings DROP COLUMN summaryLanguage; COMMIT;"

Fresh users are unaffected.
EOF
)"
```

---

## Task 2: Shared language module + strengthen language_directive

**Files:**
- Create: `frontend/src/lib/summary-languages.ts`
- Modify: `frontend/src-tauri/src/summary/processor.rs` (function `language_directive`)

- [ ] **Step 1: Create the shared TS module**

```ts
// frontend/src/lib/summary-languages.ts

export interface LanguageOption {
  code: string;
  label: string;
}

/**
 * Language options offered in the summary language pickers.
 * Codes must stay in sync with `language_name_from_code` in
 * `frontend/src-tauri/src/summary/processor.rs`.
 */
export const LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'en', label: 'English' },
  { code: 'zh', label: 'Chinese' },
  { code: 'de', label: 'German' },
  { code: 'es', label: 'Spanish' },
  { code: 'ru', label: 'Russian' },
  { code: 'ko', label: 'Korean' },
  { code: 'fr', label: 'French' },
  { code: 'ja', label: 'Japanese' },
  { code: 'pt', label: 'Portuguese' },
  { code: 'it', label: 'Italian' },
  { code: 'nl', label: 'Dutch' },
  { code: 'pl', label: 'Polish' },
  { code: 'ar', label: 'Arabic' },
  { code: 'hi', label: 'Hindi' },
  { code: 'ta', label: 'Tamil' },
  { code: 'tr', label: 'Turkish' },
  { code: 'vi', label: 'Vietnamese' },
  { code: 'th', label: 'Thai' },
  { code: 'id', label: 'Indonesian' },
  { code: 'sv', label: 'Swedish' },
];

export const AUTO_VALUE = '__auto__' as const;

export function labelForCode(code: string): string {
  return LANGUAGE_OPTIONS.find((l) => l.code === code)?.label ?? code;
}
```

- [ ] **Step 2: Update `language_name_from_code` in processor.rs**

Locate the function. Ensure regional variants are removed and `en` is present (should already be done from rev 2 alignment, but verify):

```rust
fn language_name_from_code(code: &str) -> Option<&'static str> {
    match code.to_ascii_lowercase().as_str() {
        "en" => Some("English"),
        "zh" | "zh-cn" => Some("Chinese"),
        "zh-tw" => Some("Traditional Chinese"),
        "de" => Some("German"),
        "es" => Some("Spanish"),
        "ru" => Some("Russian"),
        "ko" => Some("Korean"),
        "fr" => Some("French"),
        "ja" => Some("Japanese"),
        "pt" => Some("Portuguese"),
        "it" => Some("Italian"),
        "nl" => Some("Dutch"),
        "pl" => Some("Polish"),
        "ar" => Some("Arabic"),
        "hi" => Some("Hindi"),
        "ta" => Some("Tamil"),
        "tr" => Some("Turkish"),
        "vi" => Some("Vietnamese"),
        "th" => Some("Thai"),
        "id" => Some("Indonesian"),
        "sv" => Some("Swedish"),
        _ => None,
    }
}
```

Remove any `"en-gb"`, `"en-us"`, `"pt-br"` entries.

- [ ] **Step 3: Strengthen `language_directive` (Option A)**

Replace the existing `language_directive` body with:

```rust
fn language_directive(summary_language: Option<&str>) -> String {
    let Some(code) = summary_language else { return String::new(); };
    let Some(name) = language_name_from_code(code) else { return String::new(); };
    format!(
        "\n\n**Output language:** Produce the entire response in {}, including section headings, labels, and list markers. Do not translate proper nouns, code, file paths, or quoted material.",
        name
    )
}
```

The critical change: "including section headings, labels, and list markers" is new; "code, file paths" is slightly extended.

- [ ] **Step 4: Verify compiles**

```bash
cd frontend/src-tauri && cargo check --features metal
cd frontend && pnpm exec tsc --noEmit
```

Both expected: exit 0 / pre-existing warnings only.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/summary-languages.ts frontend/src-tauri/src/summary/processor.rs
git commit -m "$(cat <<'EOF'
feat(summary): shared language list + stronger prompt directive

- Extracts LANGUAGE_OPTIONS + labelForCode into src/lib/summary-languages.ts
  so both the Settings chip editor and the in-meeting popover can import them.
- Strengthens language_directive in processor.rs to instruct the model to
  translate section headings, labels, and list markers — fixes mixed-language
  output when built-in templates contain English section titles.
- Drops en-GB / en-US / pt-BR from language_name_from_code (bare ISO only).
EOF
)"
```

---

## Task 3: api_process_transcript summary_language param + service.rs cascade

**Files:**
- Modify: `frontend/src-tauri/src/summary/commands.rs` (function `api_process_transcript`)
- Modify: `frontend/src-tauri/src/summary/service.rs` (function `process_transcript_background`)

- [ ] **Step 1: Add `summary_language` param to `api_process_transcript`**

Modify the signature:

```rust
#[tauri::command]
pub async fn api_process_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    model: String,
    model_name: String,
    meeting_id: Option<String>,
    _chunk_size: Option<i32>,
    _overlap: Option<i32>,
    custom_prompt: Option<String>,
    template_id: Option<String>,
    summary_language: Option<String>,      // NEW
    _auth_token: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    // ...existing body...
```

Inside the body, before `tauri::async_runtime::spawn(async move { ... })`:

```rust
    // Normalise empty / whitespace-only to None
    let summary_language = summary_language.and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    });
```

And pass it to the background task:

```rust
    tauri::async_runtime::spawn(async move {
        SummaryService::process_transcript_background(
            app,
            pool,
            meeting_id_clone.clone(),
            text,
            model,
            model_name,
            final_prompt,
            final_template_id,
            summary_language,         // NEW
        )
        .await;
    });
```

- [ ] **Step 2: Update `process_transcript_background` signature**

Add `summary_language: Option<String>` to the parameter list:

```rust
pub async fn process_transcript_background<R: tauri::Runtime>(
    _app: AppHandle<R>,
    pool: SqlitePool,
    meeting_id: String,
    text: String,
    model_provider: String,
    model_name: String,
    custom_prompt: String,
    template_id: String,
    summary_language: Option<String>,     // NEW
) {
    // ...
```

- [ ] **Step 3: Replace the DB-based language fetch with the param**

Inside `process_transcript_background`, remove the block that reads from `SettingsRepository::get_summary_language` (currently around lines 222-232) and replace with a simple cascade:

```rust
    // Resolve summary output language.
    //   1. Explicit param (from frontend, which read localStorage)
    //   2. Transcription language setting (if not auto/auto-translate)
    //   3. None -- model decides from transcript content
    let summary_language: Option<String> = summary_language.or_else(|| {
        match crate::LANGUAGE_PREFERENCE.lock() {
            Ok(guard) => {
                let lang = guard.as_str();
                if lang.is_empty() || lang == "auto" || lang == "auto-translate" {
                    None
                } else {
                    Some(lang.to_string())
                }
            }
            Err(poisoned) => {
                warn!("LANGUAGE_PREFERENCE mutex poisoned: {}", poisoned);
                None
            }
        }
    });

    if let Some(code) = &summary_language {
        info!("✓ Summary language: {}", code);
    }
```

Note: the global-default tier from rev 2 is intentionally absent — the frontend resolves global default from localStorage and passes the effective value as the param. The backend only sees the resolved value plus the transcription-language fallback.

- [ ] **Step 4: Verify compiles**

```bash
cd frontend/src-tauri && cargo check --features metal
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/summary/commands.rs frontend/src-tauri/src/summary/service.rs
git commit -m "$(cat <<'EOF'
feat(summary): accept summary_language param on api_process_transcript

Frontend resolves per-meeting override → global default → Auto (matching
transcription) against localStorage and passes the effective language
as a parameter. Backend no longer touches the DB for language
preferences — it falls back only to the LANGUAGE_PREFERENCE static
(transcription language) if no param is provided.
EOF
)"
```

---

## Task 4: useRecentLanguages hook

**Files:**
- Create: `frontend/src/hooks/useRecentLanguages.ts`

- [ ] **Step 1: Create the hook**

```ts
import { useCallback, useEffect, useState } from 'react';

const MRU_KEY = 'summaryLanguageRecents';
const MAX_RECENTS = 5;

function readFromStorage(): string[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(MRU_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is string => typeof x === 'string' && x.length > 0)
      .slice(0, MAX_RECENTS);
  } catch {
    return [];
  }
}

function writeToStorage(values: string[]): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(MRU_KEY, JSON.stringify(values));
  } catch {
    // Quota exceeded / incognito — cosmetic list only, silent.
  }
}

/**
 * MRU list of recently used summary languages (max 5, localStorage).
 * Shared by SummaryLanguageSettings (chips) and LanguagePickerPopover (recents).
 *
 * addRecent: push to front, dedupe, trim to MAX_RECENTS, persist.
 */
export function useRecentLanguages() {
  const [recents, setRecents] = useState<string[]>(() => readFromStorage());

  // Cross-instance sync within same tab (chip editor open + popover mutating)
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === MRU_KEY) setRecents(readFromStorage());
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  const addRecent = useCallback((code: string) => {
    const trimmed = code.trim();
    if (!trimmed) return;
    setRecents((prev) => {
      const deduped = [trimmed, ...prev.filter((c) => c !== trimmed)].slice(0, MAX_RECENTS);
      writeToStorage(deduped);
      return deduped;
    });
  }, []);

  const removeRecent = useCallback((code: string) => {
    setRecents((prev) => {
      const updated = prev.filter((c) => c !== code);
      writeToStorage(updated);
      return updated;
    });
  }, []);

  return { recents, addRecent, removeRecent };
}
```

- [ ] **Step 2: Typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/hooks/useRecentLanguages.ts
git commit -m "$(cat <<'EOF'
feat(hooks): useRecentLanguages MRU list

localStorage-backed list (max 5) of recently used summary languages.
Shared between the Settings chip editor and the in-meeting popover.
Push-front + dedupe + trim; defensive reads handle quota / incognito.
EOF
)"
```

---

## Task 5: LanguagePickerPopover component

**Files:**
- Create: `frontend/src/components/LanguagePickerPopover.tsx`

- [ ] **Step 1: Create the component**

```tsx
'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { LANGUAGE_OPTIONS } from '@/lib/summary-languages';
import { useRecentLanguages } from '@/hooks/useRecentLanguages';

interface LanguagePickerPopoverProps {
  /** Current selection. `null` means "Auto" (unset). */
  value: string | null;
  /** Called with picked code; `null` = Auto. Caller handles closing. */
  onChange: (code: string | null) => void;
  /** Called when user clicks outside or presses Escape. */
  onClose: () => void;
  /**
   * meeting: show Recently Used + Auto option (default)
   * settings: add-language flow — no Auto, no Recently Used section
   */
  mode?: 'meeting' | 'settings';
}

export function LanguagePickerPopover({
  value,
  onChange,
  onClose,
  mode = 'meeting',
}: LanguagePickerPopoverProps) {
  const { recents } = useRecentLanguages();
  const [query, setQuery] = useState('');
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onDocClick);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDocClick);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  const filter = query.trim().toLowerCase();

  const filteredAll = useMemo(() => {
    if (!filter) return LANGUAGE_OPTIONS;
    return LANGUAGE_OPTIONS.filter(
      (l) =>
        l.code.toLowerCase().includes(filter) ||
        l.label.toLowerCase().includes(filter)
    );
  }, [filter]);

  const recentsResolved = useMemo(
    () =>
      recents
        .map((code) => LANGUAGE_OPTIONS.find((l) => l.code === code))
        .filter((l): l is (typeof LANGUAGE_OPTIONS)[number] => Boolean(l))
        .filter(
          (l) =>
            !filter ||
            l.code.toLowerCase().includes(filter) ||
            l.label.toLowerCase().includes(filter)
        ),
    [recents, filter]
  );

  const showAuto = mode === 'meeting';
  const showRecents = mode === 'meeting' && recentsResolved.length > 0;

  return (
    <div
      ref={containerRef}
      className="w-72 rounded-lg bg-white border border-gray-200 shadow-lg overflow-hidden"
      role="dialog"
      aria-label="Pick summary language"
    >
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-gray-100">
        <span className="text-gray-400 text-sm">🔍</span>
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search language..."
          className="flex-1 text-sm text-gray-900 bg-transparent border-none outline-none placeholder-gray-400"
        />
      </div>

      <div className="max-h-80 overflow-y-auto py-1">
        {showRecents && (
          <>
            <div className="px-3 pt-1 pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
              Recently Used
            </div>
            {recentsResolved.map((opt) => (
              <button
                key={`recent-${opt.code}`}
                type="button"
                onClick={() => onChange(opt.code)}
                className="flex w-full items-center justify-between px-3 py-1.5 text-sm text-gray-800 hover:bg-gray-50 text-left"
              >
                <span>{opt.label}</span>
                <span className="text-xs text-gray-400">({opt.code})</span>
              </button>
            ))}
            <div className="my-1 h-px bg-gray-100" />
          </>
        )}

        <div className="px-3 pt-1 pb-1 text-[10px] font-semibold uppercase tracking-wider text-gray-400">
          All Languages
        </div>

        {showAuto && (!filter || 'auto'.includes(filter)) && (
          <button
            type="button"
            onClick={() => onChange(null)}
            className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-left ${
              value === null ? 'text-blue-600 font-medium' : 'text-gray-800'
            }`}
          >
            <span>Auto</span>
            {value === null && <span className="text-blue-600">✓</span>}
          </button>
        )}

        {filteredAll.map((opt) => (
          <button
            key={`all-${opt.code}`}
            type="button"
            onClick={() => onChange(opt.code)}
            className={`flex w-full items-center justify-between px-3 py-1.5 text-sm hover:bg-gray-50 text-left ${
              value === opt.code ? 'text-blue-600 font-medium' : 'text-gray-800'
            }`}
          >
            <span>
              {opt.label} <span className="text-xs text-gray-400">({opt.code})</span>
            </span>
            {value === opt.code && <span className="text-blue-600">✓</span>}
          </button>
        ))}

        {filteredAll.length === 0 && !showAuto && (
          <div className="px-3 py-2 text-sm text-gray-400">No matches</div>
        )}
      </div>
    </div>
  );
}
```

Note: the popover is a **controlled component**. It does not call `addRecent` itself — the caller (`SummaryPanel` or `SummaryLanguageSettings`) owns persistence and MRU updates.

- [ ] **Step 2: Typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/LanguagePickerPopover.tsx
git commit -m "$(cat <<'EOF'
feat(ui): LanguagePickerPopover — search + recents + all

Controlled popover with always-on search, optional Recently Used
section (meeting mode only), and All Languages list. Auto is pinned
first in meeting mode. Dismisses on click-outside and Escape.
EOF
)"
```

---

## Task 6: SummaryLanguageSettings chip editor

Replace the FR-1 MVP dropdown with a chip list backed by `useRecentLanguages`, plus an "+ Add language" button that opens the popover in settings mode.

**Files:**
- Rewrite: `frontend/src/components/SummaryLanguageSettings.tsx`

- [ ] **Step 1: Write the component**

```tsx
'use client';

import { useState } from 'react';
import { LanguagePickerPopover } from '@/components/LanguagePickerPopover';
import { useRecentLanguages } from '@/hooks/useRecentLanguages';
import { labelForCode } from '@/lib/summary-languages';

/**
 * Settings card: manages the Recently Used summary languages list.
 *
 * The chips here drive the Recently Used section of the in-meeting
 * LanguagePickerPopover. Removing a chip removes it from the MRU list
 * (localStorage). There is no DB persistence and no "set as default"
 * action — the de-facto default emerges from usage via the in-meeting
 * picker, and the cascade in service.rs handles resolution.
 */
export function SummaryLanguageSettings() {
  const { recents, addRecent, removeRecent } = useRecentLanguages();
  const [pickerOpen, setPickerOpen] = useState(false);

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm relative">
      <div className="flex items-center gap-2 mb-2">
        <span className="text-gray-500">🌐</span>
        <h3 className="text-lg font-semibold text-gray-900">Summary Language</h3>
      </div>
      <p className="text-sm text-gray-600 mb-4">
        Recently used languages appear as quick-switch options in the summary
        generator. Auto always matches the transcription language.
      </p>

      <div className="flex flex-wrap items-center gap-2">
        {recents.map((code) => (
          <span
            key={code}
            className="inline-flex items-center gap-1.5 rounded-full bg-gray-100 border border-gray-200 px-3 py-1 text-sm text-gray-800"
          >
            <span>{labelForCode(code)}</span>
            <button
              type="button"
              aria-label={`Remove ${labelForCode(code)}`}
              onClick={() => removeRecent(code)}
              className="text-gray-400 hover:text-gray-700 leading-none"
            >
              ×
            </button>
          </span>
        ))}

        <button
          type="button"
          onClick={() => setPickerOpen((prev) => !prev)}
          disabled={recents.length >= 5}
          className="inline-flex items-center gap-1 rounded-full border border-dashed border-gray-300 px-3 py-1 text-sm text-gray-600 hover:border-gray-400 hover:text-gray-800 disabled:cursor-not-allowed disabled:opacity-50"
        >
          ＋ Add language
        </button>
      </div>

      <p className="text-xs text-gray-400 mt-3">
        Quick-switch options in the summary generator (max 5)
      </p>

      {pickerOpen && (
        <div className="absolute z-10 mt-2">
          <LanguagePickerPopover
            mode="settings"
            value={null}
            onChange={(code) => {
              if (code) addRecent(code);
              setPickerOpen(false);
            }}
            onClose={() => setPickerOpen(false)}
          />
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/SummaryLanguageSettings.tsx
git commit -m "$(cat <<'EOF'
feat(settings): chip editor for summary language MRU

Replaces the native <select> with a chip row backed by
useRecentLanguages. + Add language opens LanguagePickerPopover in
settings mode. Max 5 chips enforced at the UI layer. All state in
localStorage — no Tauri invokes from this card.
EOF
)"
```

---

## Task 7: SummaryPanel — language button, popover, and localStorage plumbing

Add the "🌐 ... ▾" button next to Re-generate Summary. Manage `summaryLanguage:<meetingId>` in localStorage. Pass the selected language downward so `useSummaryGeneration` can forward it to Tauri.

**Files:**
- Modify: `frontend/src/components/MeetingDetails/SummaryPanel.tsx`

- [ ] **Step 1: Add imports at the top of the file**

```tsx
import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { LanguagePickerPopover } from '@/components/LanguagePickerPopover';
import { useRecentLanguages } from '@/hooks/useRecentLanguages';
import { labelForCode } from '@/lib/summary-languages';
```

- [ ] **Step 2: Add props for summaryLanguage lifting**

Extend `SummaryPanelProps`:

```ts
interface SummaryPanelProps {
  // ...existing props...
  summaryLanguage: string | null;
  onSummaryLanguageChange: (lang: string | null) => void;
}
```

(The state actually lives in `SummaryPanel` itself — see Step 3. These props are only needed if the parent needs to know the current language for other reasons. For now, keep state local inside `SummaryPanel` and skip this step unless a parent call-site needs the value.)

**Decision for this plan:** keep state local in `SummaryPanel`. No prop lifting. `useSummaryGeneration` reads the same localStorage key directly in Task 8.

Revert this step — do not add props.

- [ ] **Step 3: Add state, effect, and handler inside the `SummaryPanel` function body**

After the destructured props block (around line 87), add:

```tsx
  const storageKey = `summaryLanguage:${meeting.id}`;
  const [summaryLang, setSummaryLang] = useState<string | null>(() => {
    if (typeof window === 'undefined') return null;
    try {
      return window.localStorage.getItem(storageKey);
    } catch {
      return null;
    }
  });
  const [langPickerOpen, setLangPickerOpen] = useState(false);
  const { addRecent } = useRecentLanguages();

  useEffect(() => {
    // Sync when the meeting changes
    if (typeof window === 'undefined') return;
    try {
      setSummaryLang(window.localStorage.getItem(storageKey));
    } catch {
      setSummaryLang(null);
    }
  }, [storageKey]);

  const handleLangChange = (code: string | null) => {
    const previous = summaryLang;
    setSummaryLang(code);
    setLangPickerOpen(false);
    try {
      if (code) {
        window.localStorage.setItem(storageKey, code);
        addRecent(code);
      } else {
        window.localStorage.removeItem(storageKey);
      }
    } catch (err) {
      console.error('Failed to persist summary language:', err);
      toast.error('Failed to save summary language');
      setSummaryLang(previous);
    }
  };
```

- [ ] **Step 4: Render the button + popover alongside the existing button groups**

Locate the block at lines 103-140 where `SummaryGeneratorButtonGroup` and `SummaryUpdaterButtonGroup` are rendered side-by-side. Add the language button as a sibling:

```tsx
            {/* Right-aligned: Summary Updater Button Group */}
            <div className="flex-shrink-0">
              <SummaryUpdaterButtonGroup
                /* ...existing props... */
              />
            </div>

            {/* Summary language picker */}
            <div className="flex-shrink-0 relative">
              <button
                type="button"
                onClick={() => setLangPickerOpen((prev) => !prev)}
                className="inline-flex items-center gap-1.5 rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-700 hover:border-gray-300 hover:bg-gray-50"
                aria-label="Set summary language"
              >
                <span>🌐</span>
                <span>{summaryLang ? labelForCode(summaryLang) : 'Auto'}</span>
                <span className="text-gray-400 text-xs">▾</span>
              </button>
              {langPickerOpen && (
                <div className="absolute right-0 mt-2 z-20">
                  <LanguagePickerPopover
                    value={summaryLang}
                    onChange={handleLangChange}
                    onClose={() => setLangPickerOpen(false)}
                  />
                </div>
              )}
            </div>
```

- [ ] **Step 5: Typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/MeetingDetails/SummaryPanel.tsx
git commit -m "$(cat <<'EOF'
feat(meeting): in-meeting summary language picker

Adds the 🌐 {label} ▾ button adjacent to the summary action groups.
Button label reflects localStorage[summaryLanguage:<meetingId>].
Selecting a language writes to localStorage + addRecent(); selecting
Auto deletes the key. No Tauri invoke — persistence is pure client-side.
EOF
)"
```

---

## Task 8: useSummaryGeneration — resolve and forward summaryLanguage

`processSummary` needs to resolve the effective language (per-meeting → global default → Auto via transcription) and pass it as `summaryLanguage` on the `api_process_transcript` invoke.

**Files:**
- Modify: `frontend/src/hooks/meeting-details/useSummaryGeneration.ts`

- [ ] **Step 1: Add a resolver helper at the top of the file (outside the hook)**

After the imports, before the `type SummaryStatus = ...` line:

```ts
function resolveSummaryLanguage(meetingId: string): string | null {
  if (typeof window === 'undefined') return null;
  try {
    const perMeeting = window.localStorage.getItem(`summaryLanguage:${meetingId}`);
    if (perMeeting) return perMeeting;

    const defaultLang = window.localStorage.getItem('summaryLanguageDefault');
    if (defaultLang) return defaultLang;

    const transcription = window.localStorage.getItem('primaryLanguage');
    if (transcription && transcription !== 'auto' && transcription !== 'auto-translate') {
      return transcription;
    }
    return null;
  } catch {
    return null;
  }
}
```

- [ ] **Step 2: Update the `invoke('api_process_transcript', ...)` call in `processSummary`**

Locate the existing invoke (around line 107). Add `summaryLanguage`:

```ts
      const summaryLanguage = resolveSummaryLanguage(meeting.id);

      // Process transcript and get process_id
      const result = await invokeTauri('api_process_transcript', {
        text: transcriptText,
        model: modelConfig.provider,
        modelName: modelConfig.model,
        meetingId: meeting.id,
        chunkSize: 40000,
        overlap: 1000,
        customPrompt: customPrompt,
        templateId: selectedTemplate,
        summaryLanguage,                 // NEW
      }) as any;
```

- [ ] **Step 3: Typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/hooks/meeting-details/useSummaryGeneration.ts
git commit -m "$(cat <<'EOF'
feat(hooks): resolve summaryLanguage in useSummaryGeneration

Reads per-meeting override, then global default, then transcription
language from localStorage. Passes the resolved code (or null) as
summaryLanguage on api_process_transcript. Regenerate inherits this
behaviour automatically via the shared processSummary path.
EOF
)"
```

---

## Task 9: Final verification

**Files:** (verification only)

- [ ] **Step 1: Full Rust build**

```bash
cd frontend/src-tauri && cargo check --features metal
```

Expected: compiles with pre-existing warnings only.

- [ ] **Step 2: Full TypeScript typecheck**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: exit 0.

- [ ] **Step 3: Dev-DB cleanup (one-off)**

Any developer who ran the FR-1 MVP migration locally must clean up:

```bash
sqlite3 ~/Library/Application\ Support/com.meetily.ai/meeting_minutes.sqlite \
  "BEGIN; DELETE FROM _sqlx_migrations WHERE version = 20260419000000; ALTER TABLE settings DROP COLUMN summaryLanguage; COMMIT;"
```

Fresh users skip this.

- [ ] **Step 4: Launch manual smoke test**

```bash
cd frontend && ./clean_run.sh
```

Checklist:
- App launches cleanly (no migration panic)
- Settings → Summary shows chip editor (no dropdown)
- "+ Add language" opens popover → pick Spanish → chip appears → × removes it
- Any meeting → "🌐 Auto ▾" button appears next to Re-generate
- Click button → popover opens, Auto has ✓
- Pick French → button label becomes "French" → reload meeting → still "French"
- Regenerate summary → Rust log shows `✓ Summary language: fr`
- Generated summary is in French **including section headings** ("## Articles d'action" etc., not "## Action Items")

- [ ] **Step 5: Tag**

```bash
git log --oneline -15
git tag -a fr1-pro-ux-complete -m "FR-1 Pro language UX complete (localStorage model)"
```

---

## Self-Review Checklist

- [x] **Spec coverage:**
  - Backend revert → Task 1
  - `api_process_transcript` summary_language param → Task 3
  - service.rs cascade → Task 3
  - language_directive strengthened (Option A) → Task 2
  - useRecentLanguages hook → Task 4
  - LanguagePickerPopover → Task 5
  - Settings chip editor → Task 6
  - In-meeting language button + localStorage → Task 7
  - useSummaryGeneration resolver → Task 8
  - Regional variants dropped → Task 2
  - Local DB cleanup documented → Task 9

- [x] **Placeholder scan:** no TBD/TODO markers; every step has actual code or commands.

- [x] **Type consistency:**
  - `LanguagePickerPopoverProps.{value, onChange, onClose, mode?}` — consistent between Task 5 definition and Tasks 6/7 usage
  - `resolveSummaryLanguage(meetingId)` — defined once in Task 8, not called elsewhere
  - `useRecentLanguages()` returns `{recents, addRecent, removeRecent}` — consistent use in Tasks 6/7
  - localStorage key conventions: `summaryLanguage:${meetingId}` and `summaryLanguageDefault` and `summaryLanguageRecents` — consistent across Tasks 7, 8, 4

---

## Learning-mode call-outs (optional user contributions)

Two locations represent real design decisions where your input would sharpen the default:

### 1. Task 4 — `addRecent` semantics

Default: exact-string dedup (`c !== trimmed`). Alternatives:
- Case-insensitive dedup (belt-and-braces if a future call site doesn't normalise)
- Code-validation guard (reject codes not in `LANGUAGE_OPTIONS`)
- Frecency weighting (vs pure MRU)

### 2. Task 5 — popover search filter

Default: substring match on both `code` and `label`. Alternatives:
- Prefix-only match (faster to type "en" for English, but "ger" won't find German)
- Fuzzy with typo tolerance (~20 extra lines)
- Language-code-first ordering when the query matches both

# Speaker Naming and Recording Playback — Design

Date: 2026-10-05
Branch: `feat/speaker-diarization` (extends `docs/specs/2026-09-27-speaker-diarization-design.md`)

## 1. Goal

Speakers get real names with as little typing as possible, and the recording can be heard in the
app so a voice can be named by ear.

1. **Voices you named before** are recognised in other meetings.
2. **Names said in the conversation** ("Noah, where are you?" … "I'm in Austin") are proposed by
   the summary model and checked in code.
3. **Playback**: play the recording from any line, follow along with the current line
   highlighted, and play a short sample of any speaker.

### Success criteria

- Naming a speaker once names the same voice in later Identify runs, and in already-identified
  meetings where that speaker is still "Speaker N" and the match is strong.
- A name you typed is never overwritten by an automatic name.
- Automatic names are visibly marked ("auto") and can be confirmed or rejected in one click; a
  rejected pair ("this is not Noah") is never proposed again.
- Automatic matches never teach a person's voice; only names you typed or confirmed do.
- "Guess names" on a meeting proposes names only with a quote from the transcript that names the
  speaker; a person who is only talked about is never applied.
- The transcript is sent to a cloud model only when the user chose a cloud model for summaries
  **and** either pressed "Guess names" or turned on auto-summary.
- Clicking a line at any point of a live recording (including after the checkpoint drift of
  about 4.6 s per hour) plays that line.
- Playback works on Linux without GStreamer's libav plugin (via the WAV clip fallback).
- Voices can be listed, renamed, merged and forgotten; "Remember voices across meetings" can be
  turned off.

### Out of scope

- Playing only one speaker's parts of a meeting.
- Waveform display.
- Editing transcript text.
- Syncing people across devices.

## 2. Decisions

| Topic | Decision |
|---|---|
| Where | Same branch as speaker identification, before the first PR. |
| Voice model | A person is the set of meeting speakers linked to them; their stored centroids are the exemplars. No new embedding storage. |
| Voice match | Strong match: applied, marked auto. Weak match: suggestion. Below: nothing. |
| Reach | Future Identify runs; past meetings only for strong matches on unnamed speakers. Renaming a person renames every linked meeting speaker. |
| Names from the conversation | Runs after Identify and fills only speakers no voice match named. Confident, quote-checked names are applied as auto; the rest are suggestions. Accepting one teaches the voice library. |
| Privacy | Automatic naming only when the summary model is local (Ollama, built-in) or auto-summary is on; otherwise only the "Guess names" button sends the transcript. |
| Voice store control | Settings → People list (rename, merge, forget one, forget all) plus "Remember voices across meetings" (default on, under the speaker identification beta). |
| Playback | Webview `<audio>` over the asset protocol; WAV clip fallback decoded in Rust when the webview cannot play the file. |
| Playback features | Play from a line, follow along, play a speaker sample. |

## 3. Data model

### 3.1 Migration `people`

```sql
CREATE TABLE IF NOT EXISTS people (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_people_name ON people (name COLLATE NOCASE);

ALTER TABLE meeting_speakers ADD COLUMN person_id TEXT REFERENCES people(id) ON DELETE SET NULL;
ALTER TABLE meeting_speakers ADD COLUMN name_source TEXT;          -- 'user' | 'voice' | 'conversation'
ALTER TABLE meeting_speakers ADD COLUMN suggested_person_id TEXT;  -- with suggested_name for new people
ALTER TABLE meeting_speakers ADD COLUMN suggested_name TEXT;
ALTER TABLE meeting_speakers ADD COLUMN suggestion_source TEXT;    -- 'voice' | 'conversation'
ALTER TABLE meeting_speakers ADD COLUMN suggestion_reason TEXT;    -- "voice match 0.68", "addressed as Noah at 01:12"

CREATE TABLE IF NOT EXISTS speaker_rejections (
    meeting_id  TEXT NOT NULL,
    speaker_key TEXT NOT NULL,
    person_id   TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_key, person_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (person_id) REFERENCES people(id) ON DELETE CASCADE
);
```

`ON DELETE SET NULL` / `CASCADE` need `PRAGMA foreign_keys = ON`; the repository also clears the
links explicitly when forgetting a person, so behaviour does not depend on the pragma.

### 3.2 Meaning

- `display_name` stays the name shown on rows and chips (denormalised, as in v1).
- **Exemplars** of a person: their meeting speakers with `name_source = 'user'` and a stored
  embedding. `voice` and `conversation` links are shown but never used as exemplars until
  confirmed (confirming sets `name_source = 'user'`).
- Typing a name on a chip means "this speaker is this person": an existing name (case-insensitive)
  links to that person, a new name creates one. Clearing the name unlinks the speaker
  (`person_id`, `name_source` and `display_name` to NULL).
- Renaming a person updates `people.name` and `display_name` of every linked meeting speaker.
- Merging person B into A relinks B's speakers to A, renames them, and deletes B.
- Forgetting a person deletes the row; linked speakers keep `display_name` but lose `person_id`
  and `name_source` becomes `user` (the name is now just text).
- "Forget all voices" does the same for every person and clears all suggestions and rejections.
- Re-running Identify on a meeting keeps v1 carry-over (previous centroids, cosine ≥ 0.6); a
  carried-over speaker keeps its `person_id`, `name_source` and suggestion.

## 4. Voice matching (`src-tauri/src/diarization/people.rs`)

- **Score(speaker, person)** = max cosine between the speaker's centroid and the person's
  exemplars. Different embedding lengths score 0 (v1 `cosine`).
- **Assignment** within one meeting: all (speaker, person) pairs above the weak threshold, best
  score first, one person per speaker and one speaker per person. Skipped: speakers with
  `name_source = 'user'`, rejected pairs, and people already linked in the meeting.
- **Strong** (≥ `VOICE_STRONG`): link, `display_name` = person name, `name_source = 'voice'`.
  **Weak** (≥ `VOICE_WEAK`): set the suggestion fields (`suggestion_source = 'voice'`, reason
  "voice match 0.68"). Below: nothing.
- **When**:
  1. At the end of every Identify job (and re-transcribe/import with speakers), after
     carry-over and before naming from the conversation, in the same transaction that writes
     the speakers.
  2. When a speaker is named or confirmed (`name_source` becomes `user`): scan every other meeting's
     speakers that are still unnamed (`display_name` NULL) for **strong** matches to that person
     and link them as `voice`. The command returns the meetings it changed so the UI can offer
     Undo, which unlinks exactly those.
- **Thresholds** `VOICE_STRONG` and `VOICE_WEAK` are measured before they are fixed: the user
  names themselves in about three meetings of the dev data; a measurement test prints the score
  distribution of same-person and different-person pairs across all meetings; the proposed values
  go to the user for approval and are recorded here with the numbers.
- **Setting off** (`remember_voices = false` in recording preferences): no matching, no
  propagation, no new exemplars. Turning it off offers "Forget all voices".

## 5. Names from the conversation (`src-tauri/src/diarization/naming.rs`)

### 5.1 Trigger

- **Automatic**: the frontend starts it after `diarization-complete` when the meeting still has an
  unnamed speaker, the setting is on, and either the summary provider is local (`ollama`,
  `builtin-ai`, or `custom-openai` whose endpoint host is `localhost`/`127.0.0.1`) or auto-summary
  is on. The rule lives in the frontend because auto-summary is a frontend setting; it is a pure,
  tested function.
- **Manual**: "Guess names" in the speaker bar, on any identified meeting.
- The auto-summary wait (v1, 120 s cap) also waits for an automatic naming run of the same meeting.
- The job runs in the existing diarization job runner as a stage `naming` ("Finding names…") so
  it shares cancellation and status.

### 5.2 Prompt

Input:
- The transcript as `[MM:SS] spk_N: text` lines (only labelled rows).
- The meeting's summary markdown, if any (attendee lists live there).
- The names of known people.

Long transcripts are cut with the summary chunker (`processor::chunk_text`) to the configured
model's context. The model is asked for JSON only:

```json
{"speakers": [{"key": "spk_2", "name": "Noah", "evidence": "Noah, where are you?",
  "kind": "addressed", "confidence": "high"}]}
```

`kind` is `self_intro`, `addressed` or `mentioned`. The call goes through the existing summary LLM
client with the user's configured provider and model.

### 5.3 Validation (in code)

A proposal is dropped unless:
- `key` is a speaker of the meeting, still unnamed and not named by a voice match, and the pair is
  not rejected;
- `name` is 1–40 characters and appears in the transcript or the summary (case-insensitive, whole
  word);
- `evidence` appears verbatim (whitespace and case normalised) in one transcript line.

Each surviving proposal is classified:
- **Applied as auto** (`name_source = 'conversation'`) when `confidence = high` and either
  `kind = self_intro` and the evidence line belongs to that speaker, or `kind = addressed`, the
  evidence line belongs to another speaker, and the named speaker speaks within the next two lines.
- **Suggestion** otherwise, except `kind = mentioned`, which is never applied but may be shown as
  a suggestion with its reason.

One name per speaker and one speaker per name: across chunks, proposals for a speaker are merged
with `self_intro` over `addressed` over `mentioned`, then by the number of verified quotes. A name
equal to a known person (case-insensitive) links that person (not an exemplar until confirmed).

### 5.4 Failure

No model configured, a network error, unparsable output or nothing valid: no changes. The
automatic run only logs; the button shows the reason.

## 6. Playback

### 6.1 Sources

- **Asset protocol**: on opening a meeting, `prepare_meeting_playback(meeting_id)` allows exactly
  that meeting's audio file in the asset protocol scope at runtime and returns its URL, duration,
  whether it is a live recording, and the time table (§6.2). The CSP gains
  `media-src 'self' asset: http://asset.localhost blob:`.
- **Fallback clips**: when `canPlayType('audio/mp4; codecs="mp4a.40.2"')` is empty or the
  `<audio>` element fails with `MEDIA_ERR_SRC_NOT_SUPPORTED`, the player switches to
  `render_playback_clip(meeting_id, start_file_s, seconds)`, which decodes that stretch and
  returns 16-bit mono 16 kHz WAV bytes as a raw IPC response. Clips are 30 s; the next clip is
  fetched before the current one ends; seeking loads the clip at that point. A developer switch
  (`localStorage['meetily.forceClipPlayback']`) forces this mode for testing.
- The unused `useAudioPlayer` hook (reads the whole file into memory) is removed.

### 6.2 Transcript time ↔ file time

Rows use the transcript clock; live recordings joined from 30 s AAC checkpoints drift from it by
about 37 ms per checkpoint. The existing `TimeMap` is exported as a piecewise-linear table of
`(clock_s, file_s)` points (identity: two points). The frontend interpolates with
`clockToFile`/`fileToClock`; both directions are unit-tested against the Rust conversion.

### 6.3 Player

- A player bar at the bottom of the transcript panel: play/pause, seek bar, current time and
  length, speed 1× / 1.5× / 2×. Space toggles playback when focus is not in a text field.
- Clicking a row's timestamp plays from that row's start.
- **Follow along**: the row containing the current clock time (binary search over loaded rows) is
  highlighted and scrolled into view. A manual scroll pauses following and shows "Back to
  playback". When playback passes the last loaded row, the next page of rows is loaded.
- **Speaker sample** (▶ on chips and speaker bar entries): plays up to 8 s from the start of that
  speaker's longest row with a single speaker, then stops.

## 7. UI

- **Chip and speaker bar**: name input with autocomplete from people; "auto" badge with ✓ confirm
  and ✕ "Not <name>"; suggestion shown as "Speaker 2 · Noah?" with ✓ / ✕; ▶ sample.
- **Speaker bar**: "Guess names" button with progress and result ("Named 3, suggested 2").
- **Toast after naming**: "Also named in N other meetings" with Undo.
- **Settings → Speakers**: "Remember voices across meetings" toggle; People list with meeting
  count and last seen; rename, merge, forget, "Forget all voices" (confirmation dialog).

## 8. Commands

| Command | Purpose |
|---|---|
| `name_speaker(meeting_id, speaker_key, name)` | Link to an existing or new person (`name_source = 'user'`); runs past-meeting propagation; returns changed meetings. |
| `confirm_speaker_name(meeting_id, speaker_key)` | Auto name or suggestion becomes `user`; runs propagation. |
| `reject_speaker_name(meeting_id, speaker_key)` | Records the rejection; clears the auto name or suggestion. |
| `undo_name_propagation(links)` | Unlinks the listed auto links. |
| `guess_speaker_names(meeting_id)` | Queues the naming stage. |
| `list_people` / `rename_person` / `merge_people` / `forget_person` / `forget_all_voices` | People management. |
| `prepare_meeting_playback(meeting_id)` | Asset scope, URL and time table. |
| `render_playback_clip(meeting_id, start_file_s, seconds)` | WAV clip fallback. |

Names follow v1's `api_` prefix (`api_name_meeting_speaker`, …). v1's `api_rename_meeting_speaker`
becomes `api_name_meeting_speaker`; `api_merge_meeting_speakers` and `api_set_transcript_speaker`
are unchanged. The `read_audio_file` command stays (other callers may use it); only the
`useAudioPlayer` hook is removed.

## 9. Testing

- **Rust unit**: people repository (link, rename everywhere, merge, forget one/all, meeting
  delete); matching (max over exemplars, one-to-one, rejections, typed names untouched, auto links
  not exemplars, past meetings strong-only on unnamed, setting off); naming (JSON in fences or with
  prose, quote grounding, name presence, `self_intro` / `addressed` / `mentioned`, one-to-one, chunk
  merge) with a fake LLM client; time table equals `TimeMap` conversion including 60 min of
  checkpoint drift; WAV clip header, length and position.
- **Frontend unit**: row at time with drift; follow-along pause and resume; speaker sample choice;
  autocomplete; the automatic-naming privacy rule; chip states (auto, suggestion, confirm, reject).
- **Measurement (ignored test)**: voice threshold calibration on the dev data (§4).
- **Manual**: name yourself in meeting A and see meeting B pick it up as auto, then Undo; "Not
  Noah" never returns; "Guess names" on SidePit finds introduced or addressed people with quotes
  and never someone only talked about; clicking a line at about 55 min plays that line; follow
  along and speaker samples; forced clip fallback (play, seek, sample); People page and the
  setting.

## 10. Known limits

- Small local models (gemma 1B) mostly yield suggestions, not auto names.
- Voices recorded on very different microphones match less well; thresholds are conservative.
- Voice data stays on the machine and can be deleted; the transcript reaches a cloud model only
  under the rule in §5.1.
- WAV clip fallback is mono 16 kHz: fine for recognising voices, not for listening quality.

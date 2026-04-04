# Technical Design v0.2.0

## Status

Implemented design baseline for `v0.2.0` completion.

This document defines:

- the intended product boundary for `v0.2.0`
- the user experience model for the core surfaces
- the architecture and data changes required to support the release
- the phased roadmap, quality bars, and exit criteria

## Release Thesis

`v0.2.0` should make MeetFree feel like one polished product:

"A local-first desktop meeting recorder and meeting note workspace for individuals who want trustworthy capture, searchable history, portable notes, and flexible local or cloud summaries."

This release should improve:

- trust in the stop/save path
- clarity of the main product surfaces
- portability of notes
- findability of past meetings
- transcript quality
- flexibility of provider configuration without turning the app into a control panel
- the quality and simplicity of the built-in local summary path

It should not try to become:

- a collaborative team workspace
- a cloud sync product
- a CRM or calendar automation layer
- a general-purpose AI dashboard

## Product Principles

The product direction for `v0.2.0` follows four principles:

### 1. Extreme Clarity

Users should always know:

- what the app is ready to do
- what state a recording is in
- what will happen when they press a primary action
- where their meeting will be after it is saved

### 2. Trust The User

MeetFree should keep power available, but it should not force power onto the screen all the time.

That means:

- smart defaults
- model and provider flexibility
- progressive disclosure for advanced options
- no patronizing "simple mode" framing

### 3. Respect Attention

Every visible element should serve the current task.

That means:

- one primary action per surface
- minimal status noise
- fewer blocking overlays
- fewer equal-weight controls competing for attention

### 4. Natural Mental Models

The interface should match what users think they are doing:

- the `Record` page should feel like a recorder
- a completed meeting should feel like a document
- the library should feel like a searchable archive

## External Signals

The direction is aligned with current product patterns in adjacent tools:

- Granola is pushing templates, meeting-history retrieval, and consent UX.
- Limitless is pushing meeting retrieval, calendar context, and export workflows.
- Superwhisper is leaning into local/offline-first workflows and user-configurable modes.
- OpenOats is exporting local markdown meeting records and indexing personal note bases.
- Krisp is emphasizing bot-free capture, searchable minutes, and vocabulary controls.

MeetFree should not copy all of those products. The signal is that users now expect:

- bot-free capture
- strong post-meeting retrieval
- portable notes
- clear privacy boundaries
- enough flexibility to match their workflow

MeetFree's best position is the combination of:

- native desktop behavior
- local-first recording and transcription
- portable meeting records
- optional but not mandatory cloud AI

## Primary User

The target user for `v0.2.0` is an individual professional who wants:

- private meeting capture
- reliable local storage
- searchable meeting history
- human-readable exported notes
- the option to use local models by default
- enough control to switch providers and models when needed
- a first-class built-in local summary engine that does not require another app to be installed

This release is still not a team collaboration product.

## Release Definition

`v0.2.0` is successful when a new user can:

1. Complete onboarding with truthful readiness states.
2. Start a meeting from the `Record` page with minimal uncertainty.
3. Stop a meeting and trust that it is fully finalized and saved.
4. Open the saved meeting immediately into a clean meeting note workspace.
5. Search across prior meetings and filter results quickly.
6. Export one or many meetings as markdown files.
7. Import an existing recording and retranscribe it as a supported workflow.
8. Improve transcript quality through cleanup and vocabulary controls.
9. Choose MeetFree Built-in, Ollama, or cloud summary providers without getting lost in settings.

## In Scope

### Core Reliability

- backend-owned meeting finalization
- durable stop/save result
- clearer shutdown progress and error states
- strong crash and interruption handling

### Core Surfaces

- refined `Record` page
- refined `Meeting` workspace
- upgraded meeting library with search and filters

### Capture And Processing

- Parakeet as default transcription path
- MeetFree Built-in as the default summary path
- Ollama as the secondary local summary option
- continued support for other summary providers
- transcript post-processing
- vocabulary controls

### Meeting Output

- markdown export for individual meetings
- batch markdown export
- optional auto-export beside meeting assets

### Supported Workflows

- live recording
- import audio
- retranscribe saved meetings
- regenerate summary

### Setup And Configuration

- truthful onboarding
- modern provider/model switching
- clearer local vs cloud configuration

## Explicitly Deferred

- collaboration and shared workspaces
- cloud sync
- calendar integrations
- PDF export
- DOCX export
- speaker diarization as a release pillar
- advanced privacy/consent automation
- template system expansion
- workflow badges in the library

## UX Model

## 1. Record Page

The current home page already functions as the live recording surface. `v0.2.0` should keep that role but clarify the idle state.

### Intent

The page should answer three questions immediately:

- `Can I record right now?`
- `What will happen when I press Record?`
- `Where will the meeting go when I stop?`

### Idle State

The idle page should contain:

- one primary `Record` action
- compact readiness indicators for:
  - microphone
  - system audio
  - transcription model
- a secondary summary readiness line
- quiet secondary actions for:
  - recover interrupted meeting
  - import audio

The idle page should not look like an empty workspace. It should look like a recorder waiting for input.

### Recording State

When recording is active:

- transcript becomes the primary content
- transport controls remain stable and visible
- state text is compact and explicit:
  - `Recording`
  - `Paused`
  - `Finalizing`
  - `Saved`
- blocking overlays are used only when the user must wait

### Non-Goals For The Record Page

- no analytics dashboard
- no library-heavy view
- no full settings surface
- no persistent provider matrix

## 2. Meeting Workspace

This is the existing meeting details surface, refined into a post-meeting document workspace.

### Intent

The meeting page should feel like opening a saved meeting note, not a control panel.

### Hierarchy

Primary content:

- summary / notes
- action items
- decisions

Secondary content:

- transcript
- retranscription controls
- generation controls

### Layout

Desktop:

- left pane: transcript reference
- main pane: summary and derived notes

Narrow screens:

- tab or segmented switch between `Notes` and `Transcript`

### Header Actions

The top-level header should include:

- title
- meeting date/time
- export markdown
- regenerate summary
- retranscribe
- open folder

### Behavior

- provider/model changes should appear in context when the user generates or regenerates summaries
- transcript remains available but visually subordinate to the note document
- markdown export is first-class

## 3. Library

The library should remain in the sidebar or evolve into a stronger left-rail list, but the behavior should improve substantially.

### Required Improvements

- fast full-text search
- date filters
- source filters:
  - recorded
  - imported
- transcript availability filter
- summary availability filter
- batch selection for export

### Non-Goals

- no badge-heavy status system
- no kanban-style organization
- no team workspace concepts

## Feature Design

## A. Reliable Capture And Finalization

### Problem

The most important workflow still crosses a fragile seam between Rust and React.

Current frontend logic in `useRecordingStop` still:

- waits for stop payloads
- coordinates some final UI state transitions
- clears transcript state
- drives navigation
- depends on event timing after the backend has already done critical work

This is directionally better than before, but `v0.2.0` should make the backend the sole owner of recording finalization.

### Goal

Stopping a recording should be a single durable operation with one authoritative final result.

### Target Architecture

#### Backend owns

- stop request handling
- waiting for final transcript workers
- transcript flush boundaries
- persistence of meeting metadata
- persistence of transcript segments
- persistence of recording assets
- persistence of finalization metadata
- durable final event payload

#### Frontend owns

- initiating stop
- rendering progress
- showing final success or failure
- navigation after success

### Target Contract

Replace the implicit multi-step stop dance with an explicit finalization result:

`stop_and_finalize_recording -> MeetingFinalizationResult`

Suggested payload:

```ts
type MeetingFinalizationResult = {
  meetingId: string | null;
  title: string;
  folderPath: string | null;
  transcriptCount: number;
  durationSeconds: number | null;
  source: "live-recording";
  transcriptionTimedOut: boolean;
  saveError: string | null;
  finalizedAt: string;
};
```

### Implementation Notes

- keep event-driven progress updates during shutdown
- add a final command result or durable completion event, but do not split persistence ownership
- ensure tray stop, shortcut stop, overlay stop, and main UI stop all use the same path
- persist enough metadata for recovery and export even when summary generation never runs

### Acceptance Criteria

- stopping from any trigger uses the same finalization path
- React no longer performs critical save orchestration
- no meeting reaches the library without a backend-generated durable record
- recovery remains available after interrupted sessions

## B. Record Page Redesign

### Goal

Make the main recording surface feel simple, calm, and trustworthy.

### Functional Requirements

- primary record button
- readiness states for microphone, system audio, and transcription
- summary readiness shown without blocking recording
- live transcript during recording
- pause/resume
- stop/finalize progress
- recovery and import as secondary actions

### Design Requirements

- idle state should contain only the actions needed before recording
- recording state should privilege transcript and transport controls
- status copy should be short and explicit
- avoid exposing model/provider settings directly on the page

### Suggested Component Structure

- `RecordPageShell`
- `RecordingReadinessCard`
- `PrimaryRecordButton`
- `RecordingTransportBar`
- `LiveTranscriptPanel`
- `SecondaryActionsRow`
- `FinalizationProgressBanner`

### Acceptance Criteria

- a first-time user can understand the page within a few seconds
- a returning user can start a recording with one click
- the page does not visually compete with the meeting workspace

## C. Meeting Workspace Refinement

### Goal

Make the saved meeting page the canonical place to review, refine, and export a meeting.

### Functional Requirements

- show summary if present
- generate summary if absent
- edit/save summary
- copy transcript
- copy summary
- retranscribe
- export markdown
- open meeting folder

### Design Requirements

- note document is primary
- transcript is reference material
- controls are contextual, not omnipresent
- export is easy to find

### Suggested Component Structure

- `MeetingHeader`
- `MeetingMetadataRow`
- `MeetingNotesPane`
- `MeetingTranscriptPane`
- `MeetingActionsMenu`
- `SummaryGenerationSheet`
- `MarkdownExportDialog`

### Acceptance Criteria

- the meeting page reads like a document first
- transcript remains accessible without crowding the note view
- the page supports export and retranscription without feeling cluttered

## D. Library Upgrade

### Goal

Make past meetings genuinely retrievable.

### Current Limitation

Transcript search currently uses case-insensitive `LIKE` matching in SQLite, which will not scale well for large histories and does not support richer filtering.

### Required Technical Change

Adopt SQLite FTS for transcript search.

### Proposed Search Capabilities

- term search across transcript text
- phrase search
- ranking by relevance and recency
- filters by:
  - date range
  - source type
  - transcript language
  - summary present

### Data Strategy

Add an FTS-backed search index synchronized with transcript persistence.

### Acceptance Criteria

- search is materially faster on larger histories
- filters compose with search
- library can drive batch export selection

## E. Provider And Model Configuration

### Goal

Make MeetFree Built-in the primary local summary experience while keeping provider flexibility and avoiding a wall of options.

### Product Stance

Defaults:

- transcription: `Parakeet`
- summary: `MeetFree Built-in`

Still supported:

- MeetFree Built-in local summary engine
- Ollama
- OpenAI
- Claude
- Groq
- OpenRouter
- custom OpenAI-compatible endpoints

### Product Stance

MeetFree should prefer a built-in local summary engine because it gives the product:

- lower setup friction
- stronger default reliability
- tighter control over model lifecycle and compatibility
- a clearer local-first story

Ollama remains valuable as a secondary local option for users who already run it or want its broader ecosystem.

### Model Strategy For MeetFree Built-in

`v0.2.0` should not promise "run any model."

It should instead support:

- a curated set of MeetFree-compatible local summary models
- hardware-aware recommendations
- optional advanced import of supported local models
- compatibility validation before a model becomes selectable

This is the product-quality version of flexibility.

### Hardware Compatibility Requirements

The built-in engine should evaluate at least:

- estimated RAM requirement
- model size / quantization
- whether the model format is supported by the local sidecar/runtime
- basic startup health and inference viability

The UI should communicate:

- `Recommended for this machine`
- `Compatible`
- `May be slow`
- `Not recommended`

### UX Approach

Settings should separate:

- `Transcription`
- `Summaries`

Within summaries, present:

- MeetFree Built-in first
- Ollama second
- cloud providers

Do not frame cloud providers as dangerous or expert-only. Present them as alternatives with clear local/cloud behavior.

### Functional Requirements

- provider selection
- model selection
- readiness state
- built-in model compatibility state
- local/cloud explanation
- secrets in OS credential store
- smooth switching without hidden state drift

### Acceptance Criteria

- MeetFree Built-in works as the default local path without extra setup
- a user can switch summary providers without confusion
- local/cloud implications are clear
- defaults remain strong without hiding other choices
- unsupported built-in models never appear as valid selectable defaults

## F. Truthful Onboarding

### Goal

Onboarding should never imply capability that is not actually ready.

### Required Behavior

- recording readiness and summary readiness are separate
- user can finish onboarding when recording is ready
- summary readiness is shown honestly if model download is still ongoing
- the first post-onboarding screen makes the current state obvious

### Acceptance Criteria

- no user reaches the record surface believing summaries are ready when they are not
- onboarding copy matches actual model readiness

## G. Import And Retranscribe

### Goal

Graduate import/retranscription from a quasi-secondary feature into a supported workflow.

### Supported Inputs

- drag and drop audio file
- import action from the main app
- retranscribe an existing meeting with a different model or language

### Required Improvements

- clearer import entry point
- clearer progress
- clearer output location and final result
- consistent error messages

### Acceptance Criteria

- import is discoverable without beta framing
- retranscription is available from the meeting workspace
- imported meetings behave like recorded meetings in the library

## H. Markdown Export

### Goal

Every saved meeting should be portable as a human-readable file.

### Why It Matters

Database-only storage is not sufficient for:

- user trust
- backup workflows
- interoperability
- long-term ownership of notes

### Export Format

Target an `OpenOats-style` markdown format with YAML frontmatter and high-signal body structure.

The public OpenOats repository added `openoats/v1` structured markdown output in March 2026. MeetFree should align with that style for interoperability, but exact parity should be validated during implementation rather than assumed.

### Required Output

- `.md` file per meeting
- YAML frontmatter
- sections:
  - `Summary`
  - `Action Items`
  - `Decisions`
  - `Transcript`

### Proposed Frontmatter

```yaml
format: meetfree/v0.2
compat: openoats/v1-inspired
meeting_id: meeting-123
title: Weekly Product Sync
created_at: 2026-04-04T11:00:00-04:00
source: live-recording
duration_seconds: 1824
language: en
transcription_provider: parakeet
summary_provider: meetfree-built-in
folder_path: /path/to/meeting
```

### Body Structure

```md
# Weekly Product Sync

## Summary
...

## Action Items
- ...

## Decisions
- ...

## Transcript
[00:00] Speaker: ...
```

### Export Modes

- single meeting export
- batch export from filtered library selection
- optional auto-export beside meeting assets

### Acceptance Criteria

- markdown export works even if no summary exists yet
- auto-export does not block recording finalization
- exported files remain readable without the app

## I. Transcript Post-Processing

### Goal

Improve transcript readability without destroying user trust.

### Important Product Rule

Cleanup should improve readability, but users must never feel the app is rewriting their meeting irresponsibly.

### Proposed Model

Store or derive both:

- `raw transcript`
- `cleaned transcript`

Use cases:

- display cleaned by default
- export cleaned by default
- preserve raw transcript for debugging, audit, and future tuning

### Required Cleanup Features

- punctuation correction
- capitalization correction
- whitespace normalization
- sentence boundary improvement
- optional filler word removal

### Nice-To-Have Within Scope Only If Low Risk

- light speaker label cleanup where data already exists

### Explicitly Not Required

- true diarization-based speaker attribution

### Current Codebase Note

There is already a backend post-processor that performs:

- filler cleanup
- punctuation normalization
- capitalization improvement
- repetitive text cleanup

`v0.2.0` should productize and integrate that pipeline rather than leaving it as an incomplete capability.

### Acceptance Criteria

- saved transcripts are visibly cleaner than raw ASR output
- filler removal is optional
- no significant transcript corruption is introduced

## J. Accuracy Controls

### Goal

Let users teach MeetFree their language without building a full knowledge base product.

### Product Shape

This is not OpenOats-style document retrieval.

For MeetFree, `accuracy controls` should mean:

- vocabulary glossary
- preferred spellings
- acronym expansion rules
- name correction rules
- optional meeting-level context hints

### Example Entries

- `meet free -> MeetFree`
- `olama -> Ollama`
- `meet free built in -> MeetFree Built-in`
- `sock 2 -> SOC 2`
- `axiom -> Axiom`

### Where It Applies

- post-processing correction
- retranscription correction
- export rendering

### Storage Model

Add a user-editable vocabulary table with optional scopes:

- global
- per meeting

### Acceptance Criteria

- users can define domain terminology
- exported and displayed transcripts reflect those corrections
- the feature remains lightweight and understandable

## Data Model Changes

## 1. Meetings Table

Add fields as needed for the release:

- `source_type` (`live`, `imported`)
- `language`
- `duration_seconds`
- `recording_started_at`
- `recording_ended_at`
- `markdown_export_path` nullable

## 2. Transcripts Table

Support raw and cleaned output:

- `raw_transcript`
- `cleaned_transcript`
- `processing_version`

If schema churn is a concern, keep the current transcript text column as the cleaned transcript and add only `raw_transcript`.

## 3. Vocabulary Table

Suggested table:

- `id`
- `scope_type` (`global`, `meeting`)
- `scope_id` nullable
- `source_text`
- `target_text`
- `case_sensitive`
- `created_at`
- `updated_at`

## 4. Export Jobs Table

Optional but useful if batch export becomes asynchronous:

- `id`
- `created_at`
- `status`
- `filter_json`
- `output_directory`

## 5. Search Index

Add an FTS table synchronized with transcript persistence.

## Architecture Changes

## 1. Stop/Finalize Ownership

Move the critical save path fully into Rust.

Key files likely impacted:

- `desktop/src-tauri/src/audio/recording_commands.rs`
- `desktop/src/hooks/useRecordingStop.ts`
- `desktop/src/contexts/RecordingPostProcessingProvider.tsx`
- `desktop/src/contexts/TranscriptContext.tsx`

## 2. Search Layer

Replace transcript `LIKE` search with FTS-based queries.

Key files likely impacted:

- `desktop/src-tauri/src/database/repositories/transcript.rs`
- `desktop/src-tauri/src/api/api.rs`
- `desktop/src/components/Sidebar/SidebarProvider.tsx`

## 3. Export Layer

Add a dedicated markdown export service.

Suggested module:

- `desktop/src-tauri/src/export/`

Suggested responsibilities:

- markdown rendering
- YAML frontmatter generation
- file naming
- batch export orchestration
- auto-export hooks from finalization

## 4. Post-Processing Layer

Promote the existing post-processor into a real transcript pipeline stage.

Key files likely impacted:

- `desktop/src-tauri/src/audio/post_processor.rs`
- transcription worker integration
- transcript persistence path

## 5. Settings Simplification

Refactor the settings UI so it remains flexible but clearer.

Key files likely impacted:

- `desktop/src/components/ModelSettingsModal.tsx`
- `desktop/src/components/SummaryModelSettings.tsx`
- `desktop/src/components/TranscriptSettings.tsx`
- `desktop/src/app/settings/page.tsx`

## 6. Page Surface Refactors

Key files likely impacted:

- `desktop/src/app/page.tsx`
- `desktop/src/app/_components/TranscriptPanel.tsx`
- `desktop/src/app/meeting-details/page-content.tsx`
- `desktop/src/components/MeetingDetails/TranscriptPanel.tsx`
- `desktop/src/components/MeetingDetails/SummaryPanel.tsx`

## API And Command Surface

## New Or Revised Commands

Suggested additions:

- `stop_and_finalize_recording`
- `meeting_export_markdown`
- `meetings_export_markdown_batch`
- `meeting_markdown_preview`
- `vocabulary_list`
- `vocabulary_upsert`
- `vocabulary_delete`
- `transcript_postprocess_preview`

Suggested revised behavior:

- `stop_recording` should either become a pure low-level stop or be replaced by the finalization command to avoid split ownership

## Testing Strategy

## Unit Tests

- transcript cleanup transformations
- vocabulary correction logic
- markdown renderer
- YAML frontmatter serialization
- FTS search queries
- export file naming and collision handling

## Integration Tests

- record -> stop -> finalize -> saved meeting
- import -> retranscribe -> saved meeting
- summary generation -> markdown export
- auto-export on meeting finalization

## Recovery Tests

- interrupted session recovery
- save timeout handling
- partial transcription completion
- export failure should not corrupt meeting state

## UI Tests

- onboarding readiness truthfulness
- record page idle clarity
- meeting workspace export flow
- provider switching flow
- library search and filters

## Release Quality Bars

`v0.2.0` should not ship unless:

- finalization is backend-owned and durable
- live recording and import both end in the same stable meeting model
- library search is materially better than current substring search
- markdown export is reliable and readable
- transcript cleanup improves quality without obvious corruption
- onboarding truthfully communicates readiness
- documentation matches implementation

## Phased Roadmap

## Phase 0: Design Lock And Spikes

### Goals

- lock product scope
- validate markdown export shape
- validate FTS migration path
- validate finalization contract

### Tasks

- confirm markdown frontmatter/body shape against current OpenOats public format
- decide whether to store raw + cleaned transcript or cleaned + derived raw reference
- design finalization result contract
- design library filters and batch export flow
- design vocabulary schema and correction rules
- define exact `Record` and `Meeting` page wireframes

### Exit Criteria

- open product questions resolved
- no major schema ambiguity remains
- implementation order approved

## Phase 1: Reliability Foundation

### Goals

- make stop/save fully backend-owned
- harden the data model for export and search

### Tasks

- implement finalization command/result
- simplify frontend stop flow to status + navigation
- add schema changes for meeting metadata
- add transcript raw/cleaned persistence strategy
- add search index migration
- add tests for finalization and recovery

### Exit Criteria

- all stop paths use the same backend-owned finalization
- meeting records are durably created before frontend cleanup
- regression tests cover stop/save/recovery

## Phase 2: Core Surface Redesign

### Goals

- redesign `Record` page
- redesign `Meeting` workspace
- keep settings flexible but calmer

### Tasks

- implement refined idle `Record` page
- refine live transcript recording view
- refine meeting workspace hierarchy
- add export action entry points
- move provider/model changes into better contextual UI
- simplify settings IA for transcription and summaries

### Exit Criteria

- main surfaces are visually simpler than `v0.1.0`
- power remains accessible without dominating the UI
- first-time recording flow feels obvious

## Phase 3: Search, Export, And Supported Import

### Goals

- ship the portable note story
- make history retrieval strong
- graduate import/retranscribe

### Tasks

- implement FTS-backed search and filters
- implement single-meeting markdown export
- implement batch export
- implement auto-export option
- graduate import UI and retranscription UX
- ensure imported meetings are first-class in the library

### Exit Criteria

- users can search and filter a large history effectively
- exported markdown is stable and readable
- import/retranscribe no longer feels experimental

## Phase 4: Transcript Quality And Accuracy Controls

### Goals

- improve transcript readability
- let users teach MeetFree their terminology

### Tasks

- wire post-processor into saved transcript pipeline
- expose cleanup preferences in settings
- add vocabulary management UI
- apply vocabulary corrections to export and meeting display
- validate low-risk sentence cleanup behavior

### Exit Criteria

- transcript readability improves measurably
- vocabulary corrections work end to end
- users can opt out of aggressive cleanup

## Phase 5: Release Hardening

### Goals

- stabilize behavior
- align docs
- finish release validation

### Tasks

- cross-platform validation for recording, export, and import
- migration testing on real user databases
- performance checks on large transcript histories
- documentation updates
- release notes and packaging validation

### Exit Criteria

- release checklist completed
- docs and implementation aligned
- no unresolved high-severity reliability bugs

## Suggested Execution Order

1. Finalization contract and backend ownership
2. Schema changes and FTS groundwork
3. Record page redesign
4. Meeting workspace refinement
5. Markdown export
6. Import/retranscribe graduation
7. MeetFree Built-in model management and compatibility UX
8. Transcript cleanup and vocabulary controls
9. Hardening and migration validation

## Decisions Locked

- Single-meeting markdown export defaults to the meeting folder; batch export uses a chosen root with per-meeting subfolders.
- Raw transcript text is retained for audit/debug storage and not shown by default in UI.
- Auto-export runs asynchronously after finalize success and does not block finalization.
- Vocabulary corrections apply to transcript display, summary-generation input, and markdown export.
- Supported built-in models are curated and hardware-ranked with recommendation/compatibility metadata.
- Advanced model import is allowed only for supported built-in model names and validated GGUF files copied into the managed app models directory.

## Bottom Line

`v0.2.0` should not be a feature scatter release.

It should be the release that makes MeetFree:

- dependable to stop and save
- simple to start using
- pleasant to review after a meeting
- easy to search later
- portable outside the app

That is the most coherent next step for the product and the codebase.

# Execution Plan v0.2.0

## Purpose

This document turns the `v0.2.0` technical design into an implementation plan with:

- milestones
- ticket-sized work items
- dependency order
- file and module ownership
- acceptance criteria

It is intended to be used directly for sprint planning and implementation sequencing.

For product goals and architecture rationale, see:

- `docs/technical-design-v0.2.0.md`

## Planning Assumptions

- The product of record remains the native desktop app under `desktop/`.
- `v0.2.0` is still a single-user local-first release.
- Reliability work must land before large UX surface changes.
- Search and export foundations should land before major library/workspace polish.
- The execution plan uses ownership by module / write scope, not by person.

## Ownership Model

Use these owner labels when assigning work:

- `R-Audio`: Rust recording, transcription, finalization, post-processing
- `R-DB`: Rust migrations, repositories, persistence
- `R-API`: Rust Tauri command surface and DTOs
- `R-Export`: Rust markdown export rendering and file output
- `F-Record`: React record page, recording state, transcript live view
- `F-Meeting`: React meeting details workspace, export UI, retranscription UI
- `F-Library`: React sidebar/library, search, filters, batch selection
- `F-Settings`: React onboarding, model settings, transcript preferences, vocabulary UI
- `QA-Core`: deterministic tests, migration tests, end-to-end validation

## Milestone Overview

### M0. Design Lock And Technical Spikes

Goal:

- lock key contracts before invasive implementation starts

### M1. Finalization Path

Goal:

- backend-owned, durable stop/save flow

### M2. Search Foundation

Goal:

- SQLite FTS and stronger library search/filter API

### M3. Markdown Export Foundation

Goal:

- single-meeting export, export renderer, file output contract

### M4. Core Surface Refinement

Goal:

- simplified `Record` page and refined `Meeting` workspace

### M5. Import/Retranscribe Graduation

Goal:

- treat import and retranscribe as supported workflows

### M6. Transcript Cleanup And Accuracy Controls

Goal:

- cleaned saved transcripts and vocabulary correction support

### M7. Onboarding And Settings Simplification

Goal:

- truthful readiness and cleaner provider/model switching

### M7A. MeetFree Built-in Model Management

Goal:

- make the built-in local summary engine the best supported local path

### M8. Release Hardening

Goal:

- migration confidence, performance, validation, documentation

## Dependency Graph

High-level order:

1. `M0` design lock
2. `M1` finalization path
3. `M2` search foundation
4. `M3` markdown export foundation
5. `M4` core surface refinement
6. `M5` import/retranscribe graduation
7. `M6` transcript cleanup and accuracy controls
8. `M7` onboarding and settings simplification
9. `M8` release hardening

Important dependencies:

- `M4` depends on `M1` because the record surface should render the new finalization model, not the old one.
- `M3` depends partly on `M1` because export should use durable meeting metadata.
- `M4` meeting workspace export UX depends on `M3`.
- `M6` vocabulary correction should build on the stable persistence path from `M1`.
- `M7` truthful onboarding depends on final provider/readiness behavior.
- `M7A` depends on the provider/model direction being locked before the final settings UX is refined.

## M0. Design Lock And Technical Spikes

## V2-0001 Finalization Contract Spike

Owner:

- `R-Audio`
- `R-API`
- `F-Record`

Primary Files:

- `desktop/src-tauri/src/audio/recording_commands.rs`
- `desktop/src-tauri/src/api/api.rs`
- `desktop/src/services/recordingService.ts`
- `desktop/src/hooks/useRecordingStop.ts`

Task:

- define the final durable stop/finalization payload
- decide whether the frontend waits on a command result, a completion event, or both
- document the exact status/state transitions

Deliverable:

- written contract in code comments or design notes
- agreed payload fields and naming

Acceptance Criteria:

- all teams agree on one source of truth for recording finalization

## V2-0002 Search Schema Spike

Owner:

- `R-DB`
- `R-API`
- `F-Library`

Primary Files:

- `desktop/src-tauri/migrations/`
- `desktop/src-tauri/src/database/repositories/transcript.rs`
- `desktop/src-tauri/src/api/api.rs`
- `desktop/src/components/Sidebar/SidebarProvider.tsx`

Task:

- define FTS table shape
- decide snippet generation strategy
- define search filters and API response shape

Acceptance Criteria:

- FTS migration plan is implementation-ready

## V2-0003 Markdown Export Format Spike

Owner:

- `R-Export`
- `R-DB`
- `F-Meeting`

Primary Files:

- new `desktop/src-tauri/src/export/` module
- `desktop/src-tauri/src/summary/contract.rs`
- `desktop/src/app/meeting-details/page-content.tsx`

Task:

- confirm export shape
- map summary/no-summary cases
- define file naming and auto-export behavior

Acceptance Criteria:

- markdown export contract is explicit enough to implement without ambiguity

## M1. Finalization Path

This milestone is the first implementation milestone and the critical path for `v0.2.0`.

## V2-1001 Introduce Durable Finalization Result Type

Owner:

- `R-Audio`
- `R-API`

Primary Files:

- `desktop/src-tauri/src/audio/recording_commands.rs`
- `desktop/src-tauri/src/api/meetings.rs`
- `desktop/src-tauri/src/command_registry.rs`

Touched Files:

- `desktop/src/services/recordingService.ts`

Task:

- add a canonical `MeetingFinalizationResult` Rust type
- expose it through a dedicated command/event surface
- stop treating `recording-stopped` as a loosely shaped metadata message

Suggested Output Fields:

- `meeting_id`
- `meeting_title`
- `folder_path`
- `transcript_count`
- `duration_seconds`
- `transcription_timed_out`
- `save_error`
- `finalized_at`
- `source_type`

Acceptance Criteria:

- one typed result represents successful or failed finalization

## V2-1002 Collapse Stop + Save Into One Backend-Owned Flow

Owner:

- `R-Audio`

Primary Files:

- `desktop/src-tauri/src/audio/recording_commands.rs`
- `desktop/src-tauri/src/audio/recording_manager.rs`
- `desktop/src-tauri/src/audio/incremental_saver.rs`

Task:

- make the backend own the full stop/finalize sequence
- remove any remaining expectation that the frontend coordinates persistence timing
- ensure all stop entry points use the same internal function

Acceptance Criteria:

- tray stop, UI stop, shortcut stop, and overlay stop all route through the same finalization path

## V2-1003 Simplify Frontend Stop Handling To Render-Only

Owner:

- `F-Record`

Primary Files:

- `desktop/src/hooks/useRecordingStop.ts`
- `desktop/src/contexts/RecordingPostProcessingProvider.tsx`
- `desktop/src/services/recordingService.ts`

Touched Files:

- `desktop/src/app/page.tsx`
- `desktop/src/contexts/RecordingStateContext.tsx`

Task:

- remove frontend-critical save orchestration
- keep only:
  - stop initiation
  - finalization progress display
  - success/failure messaging
  - navigation after completion

Acceptance Criteria:

- React no longer performs save ownership work after stop

## V2-1004 Clean Up TranscriptContext Save Coupling

Owner:

- `F-Record`
- `R-Audio`

Primary Files:

- `desktop/src/contexts/TranscriptContext.tsx`
- `desktop/src/services/storageService.ts`

Task:

- remove or reduce IndexedDB and sessionStorage coupling that exists only to support the old split save flow
- preserve crash recovery, but make recovery metadata depend on backend-owned finalization semantics

Acceptance Criteria:

- transcript recovery still works
- save path is no longer dependent on `markMeetingAsSaved()` semantics for correctness

## V2-1005 Persist Additional Meeting Finalization Metadata

Owner:

- `R-DB`
- `R-Audio`

Primary Files:

- `desktop/src-tauri/migrations/`
- `desktop/src-tauri/src/database/repositories/meeting.rs`
- `desktop/src-tauri/src/database/repositories/transcript.rs`
- `desktop/src-tauri/src/audio/recording_commands.rs`

Task:

- add fields required by export and library evolution:
  - `source_type`
  - `duration_seconds`
  - `recording_started_at`
  - `recording_ended_at`
  - `language` if available

Acceptance Criteria:

- every finalized meeting contains the metadata required by later milestones

## V2-1006 Finalization Regression Test Suite

Owner:

- `QA-Core`
- `R-Audio`
- `R-DB`

Primary Files:

- Rust tests adjacent to recording/finalization modules
- migration tests if required

Task:

- add tests for:
  - normal stop
  - transcription timeout
  - backend save failure
  - interrupted session recovery handoff

Acceptance Criteria:

- finalization path is covered by deterministic tests

## M1 Exit Criteria

- stop/finalize is backend-owned
- frontend stop path is render-only
- meeting metadata is durable enough for export and search
- no known high-severity data-loss regressions remain

## M2. Search Foundation

This milestone should land immediately after `M1`.

## V2-2001 Add FTS Migration And Search Index Schema

Owner:

- `R-DB`

Primary Files:

- `desktop/src-tauri/migrations/`
- `desktop/src-tauri/src/database/manager.rs`

Task:

- add SQLite FTS virtual table for transcript search
- decide sync strategy between `transcripts` and FTS table
- add indexes needed for meeting filters

Acceptance Criteria:

- existing databases migrate cleanly
- new databases create the FTS structure automatically

## V2-2002 Synchronize Transcript Writes Into FTS

Owner:

- `R-DB`

Primary Files:

- `desktop/src-tauri/src/database/repositories/transcript.rs`

Touched Files:

- `desktop/src-tauri/src/database/repositories/meeting.rs`

Task:

- update transcript persistence so transcript inserts/updates populate the FTS index
- ensure retranscription paths reindex correctly

Acceptance Criteria:

- recorded meetings, imported meetings, and retranscribed meetings all become searchable

## V2-2003 Replace LIKE Search API With FTS Query API

Owner:

- `R-API`
- `R-DB`

Primary Files:

- `desktop/src-tauri/src/database/repositories/transcript.rs`
- `desktop/src-tauri/src/api/api.rs`
- `desktop/src-tauri/src/api/meetings.rs`

Task:

- replace current `LIKE` search implementation
- return better context snippets
- add structured filters to the API

Suggested Filter Inputs:

- `query`
- `date_from`
- `date_to`
- `source_type`
- `has_summary`

Acceptance Criteria:

- API supports fast search and filter composition

## V2-2004 Expand Library Search State Model In React

Owner:

- `F-Library`

Primary Files:

- `desktop/src/components/Sidebar/SidebarProvider.tsx`
- `desktop/src/components/Sidebar/index.tsx`

Task:

- replace current query-only search state with a richer library search model
- support filters alongside transcript search
- preserve current meeting navigation behavior

Acceptance Criteria:

- library search state is ready for richer UI without hacks

## V2-2005 Build Minimal Filter UI

Owner:

- `F-Library`

Primary Files:

- `desktop/src/components/Sidebar/index.tsx`

Task:

- add minimal, uncluttered filters
- avoid badge-heavy, noisy UI
- prioritize:
  - date
  - source
  - summary present

Acceptance Criteria:

- search remains simple
- filters are available without turning the sidebar into a dashboard

## V2-2006 Search Performance And Migration Validation

Owner:

- `QA-Core`
- `R-DB`

Task:

- validate migration on existing databases
- validate search performance on a realistic meeting corpus

Acceptance Criteria:

- search is materially better than current substring search

## M2 Exit Criteria

- FTS is live
- library search/filter API is stable
- sidebar can drive future batch export selection

## M3. Markdown Export Foundation

This milestone should ship the first portable-note foundation.

## V2-3001 Create Export Module Skeleton

Owner:

- `R-Export`

Primary Files:

- new `desktop/src-tauri/src/export/mod.rs`
- new `desktop/src-tauri/src/export/markdown.rs`
- new `desktop/src-tauri/src/export/frontmatter.rs`
- `desktop/src-tauri/src/lib.rs` or module registration path

Task:

- create a dedicated Rust export module
- keep markdown export logic separate from summary generation logic

Acceptance Criteria:

- export has a clear home in the backend

## V2-3002 Define Markdown Export DTO And File Naming Rules

Owner:

- `R-Export`
- `R-API`

Primary Files:

- `desktop/src-tauri/src/export/markdown.rs`
- `desktop/src-tauri/src/api/api.rs`

Task:

- define renderer input DTO
- implement filename normalization and collision handling
- choose path conventions for single export and auto-export

Acceptance Criteria:

- export naming is deterministic and safe

## V2-3003 Implement Single-Meeting Markdown Export

Owner:

- `R-Export`
- `R-DB`

Primary Files:

- `desktop/src-tauri/src/export/markdown.rs`
- `desktop/src-tauri/src/database/repositories/meeting.rs`
- `desktop/src-tauri/src/database/repositories/transcript.rs`
- `desktop/src-tauri/src/database/repositories/summary.rs`

Task:

- render `.md` output with YAML frontmatter
- support:
  - summary present
  - summary absent
  - transcript only

Required Body Sections:

- `Summary`
- `Action Items`
- `Decisions`
- `Transcript`

Acceptance Criteria:

- any saved meeting can be exported to a readable markdown file

## V2-3004 Add Tauri Command Surface For Export

Owner:

- `R-API`

Primary Files:

- `desktop/src-tauri/src/api/meetings.rs`
- `desktop/src-tauri/src/command_registry.rs`

Task:

- add commands for:
  - single meeting export
  - export preview or path return if needed

Acceptance Criteria:

- frontend can trigger export with a stable command contract

## V2-3005 Add Meeting Workspace Export Action

Owner:

- `F-Meeting`

Primary Files:

- `desktop/src/app/meeting-details/page-content.tsx`
- `desktop/src/components/MeetingDetails/SummaryPanel.tsx`
- new export trigger component if needed

Task:

- add export action to the meeting workspace header or action menu
- show success/failure feedback and destination path

Acceptance Criteria:

- markdown export is a first-class user action on a saved meeting

## V2-3006 Batch Export Foundation

Owner:

- `R-Export`
- `F-Library`

Primary Files:

- export module
- library selection state

Task:

- add backend and frontend support for exporting multiple meetings from a library selection
- keep UI simple; do not over-design this in the first pass

Acceptance Criteria:

- filtered or selected meetings can be exported together

## V2-3007 Auto-Export Option

Owner:

- `R-Export`
- `F-Settings`

Primary Files:

- export module
- recording finalization path
- settings UI / config storage

Task:

- add optional auto-export on finalization
- ensure export failures do not make finalization look like a failed save

Acceptance Criteria:

- auto-export is optional and non-blocking

## V2-3008 Export Test Coverage

Owner:

- `QA-Core`
- `R-Export`

Task:

- add tests for:
  - frontmatter generation
  - missing summary cases
  - filename collisions
  - batch export edge cases

Acceptance Criteria:

- export output is deterministic and safe

## M3 Exit Criteria

- single-meeting export ships
- batch export foundation exists
- auto-export is possible without compromising finalization reliability

## M4. Core Surface Refinement

## V2-4001 Refine Record Page Idle State

Owner:

- `F-Record`

Primary Files:

- `desktop/src/app/page.tsx`
- `desktop/src/app/_components/TranscriptPanel.tsx`
- `desktop/src/components/RecordingControls.tsx`

Task:

- redesign idle state around one primary action and readiness clarity

## V2-4002 Refine Live Recording Surface

Owner:

- `F-Record`

Primary Files:

- `desktop/src/app/page.tsx`
- `desktop/src/components/RecordingControls.tsx`
- `desktop/src/app/_components/StatusOverlays.tsx`

Task:

- reduce status clutter
- improve finalization progress presentation

## V2-4003 Refine Meeting Workspace Hierarchy

Owner:

- `F-Meeting`

Primary Files:

- `desktop/src/app/meeting-details/page-content.tsx`
- `desktop/src/components/MeetingDetails/TranscriptPanel.tsx`
- `desktop/src/components/MeetingDetails/SummaryPanel.tsx`

Task:

- make the notes document primary
- keep transcript as supporting reference

## M5. Import/Retranscribe Graduation

## V2-5001 Remove Beta Framing From Import

Owner:

- `F-Record`
- `F-Settings`

Primary Files:

- `desktop/src/components/BetaSettings.tsx`
- `desktop/src/app/layout.tsx`
- import entry-point components

## V2-5002 Improve Import Entry And Progress

Owner:

- `F-Record`
- `R-Audio`

Primary Files:

- `desktop/src/components/ImportAudio/`
- `desktop/src-tauri/src/audio/import.rs`

## V2-5003 Make Retranscription A Standard Meeting Action

Owner:

- `F-Meeting`
- `R-Audio`

Primary Files:

- `desktop/src/components/MeetingDetails/RetranscribeDialog.tsx`
- `desktop/src-tauri/src/audio/retranscription.rs`

## M6. Transcript Cleanup And Accuracy Controls

## V2-6001 Wire Saved Transcript Cleanup Pipeline

Owner:

- `R-Audio`
- `R-DB`

Primary Files:

- `desktop/src-tauri/src/audio/post_processor.rs`
- transcription worker integration
- transcript repository persistence

## V2-6002 Add Transcript Cleanup Settings

Owner:

- `F-Settings`

Primary Files:

- `desktop/src/components/TranscriptSettings.tsx`
- config state/store

## V2-6003 Add Vocabulary Persistence

Owner:

- `R-DB`
- `R-API`

Primary Files:

- migrations
- new repository or setting submodule
- command surface

## V2-6004 Add Vocabulary UI

Owner:

- `F-Settings`

Primary Files:

- settings page and components

## V2-6005 Apply Vocabulary Corrections To Export And Display

Owner:

- `R-Audio`
- `R-Export`
- `F-Meeting`

## M7. Onboarding And Settings Simplification

## V2-7001 Truthful Onboarding Readiness Copy

Owner:

- `F-Settings`
- `R-API`

Primary Files:

- onboarding steps
- readiness APIs

## V2-7002 Clean Provider / Model Switching UX

Owner:

- `F-Settings`

Primary Files:

- `desktop/src/components/ModelSettingsModal.tsx`
- `desktop/src/components/SummaryModelSettings.tsx`
- `desktop/src/components/TranscriptSettings.tsx`

Task:

- present MeetFree Built-in as the primary local summary path
- present Ollama as the secondary local option
- keep cloud providers available without cluttering the default flow

Acceptance Criteria:

- the settings IA clearly privileges MeetFree Built-in without hiding alternatives

## M7A. MeetFree Built-in Model Management

This milestone makes the built-in engine a real product pillar rather than a hidden implementation detail.

## V2-7101 Rename And Reframe Built-in Provider In The UI

Owner:

- `F-Settings`
- `F-Meeting`
- `F-Record`

Primary Files:

- `desktop/src/components/ModelSettingsModal.tsx`
- `desktop/src/components/SummaryModelSettings.tsx`
- onboarding-related UI
- any provider labels in meeting generation UI

Task:

- change user-facing naming from generic `Built-in AI` toward `MeetFree Built-in` or equivalent final label
- make it clear this is the default local summary engine managed by MeetFree

Acceptance Criteria:

- user-facing naming clearly communicates what the provider is

## V2-7102 Add Built-in Model Compatibility Metadata

Owner:

- `R-Export`
- `R-API`
- `R-Audio`

Primary Files:

- `desktop/src-tauri/src/summary/summary_engine/commands.rs`
- `desktop/src-tauri/src/summary/summary_engine/models.rs`
- `desktop/src-tauri/src/summary/summary_engine/model_manager.rs`

Task:

- extend built-in model listing to include compatibility and recommendation metadata
- expose at least:
  - estimated memory requirement
  - size
  - compatibility state
  - recommendation state

Acceptance Criteria:

- frontend can render meaningful model cards without guessing

## V2-7103 Add Hardware-Aware Built-in Model Recommendation Logic

Owner:

- `R-Audio`
- `R-API`

Primary Files:

- `desktop/src-tauri/src/summary/summary_engine/commands.rs`
- hardware/system capability helpers already in the repo where relevant

Task:

- recommend built-in local models by hardware tier
- ensure onboarding and settings use the same recommendation logic

Acceptance Criteria:

- built-in model recommendation is deterministic and shared across the app

## V2-7104 Curated Built-in Model Manager UX

Owner:

- `F-Settings`

Primary Files:

- `desktop/src/components/BuiltInModelManager.tsx`
- `desktop/src/components/ModelSettingsModal.tsx`
- `desktop/src/components/SummaryModelSettings.tsx`

Task:

- make MeetFree Built-in model selection the primary local-model workflow
- show recommendation and compatibility states such as:
  - recommended
  - compatible
  - may be slow
  - not recommended

Acceptance Criteria:

- built-in model selection feels like a supported product flow, not a backend utility

## V2-7105 Optional Advanced Import For Supported Built-in Models

Owner:

- `R-Audio`
- `R-API`
- `F-Settings`

Primary Files:

- built-in summary engine model manager modules
- settings UI

Task:

- if included in `v0.2.0`, allow advanced users to add supported built-in local models
- validate model format and compatibility before activation

Acceptance Criteria:

- advanced extensibility does not weaken default model safety

## V2-7106 Make MeetFree Built-in The Default Onboarding Summary Path

Owner:

- `F-Settings`
- `R-API`

Primary Files:

- onboarding UI
- `desktop/src-tauri/src/onboarding.rs`
- `desktop/src-tauri/src/database/commands.rs`

Task:

- keep MeetFree Built-in as the default recommended local summary provider
- ensure onboarding explains Ollama as an alternative local runtime, not the primary path

Acceptance Criteria:

- onboarding and settings tell the same local-summary story

## M8. Release Hardening

## V2-8001 Migration Validation

Owner:

- `QA-Core`
- `R-DB`

## V2-8002 Performance Validation

Owner:

- `QA-Core`
- `R-DB`
- `F-Library`

## V2-8003 Cross-Platform Export / Import / Record Validation

Owner:

- `QA-Core`
- relevant module owners

## V2-8004 Documentation Alignment

Owner:

- product/engineering docs owner

Primary Files:

- `README.md`
- `docs/technical-design-v0.2.0.md`
- release notes and any updated docs

## Suggested Sprint Packaging

### Sprint A

- `V2-0001`
- `V2-0002`
- `V2-0003`
- `V2-1001`
- `V2-1002`

### Sprint B

- `V2-1003`
- `V2-1004`
- `V2-1005`
- `V2-1006`
- `V2-2001`

### Sprint C

- `V2-2002`
- `V2-2003`
- `V2-2004`
- `V2-2005`
- `V2-2006`

### Sprint D

- `V2-3001`
- `V2-3002`
- `V2-3003`
- `V2-3004`
- `V2-3005`

### Sprint E

- `V2-3006`
- `V2-3007`
- `V2-3008`
- `V2-4001`
- `V2-4002`

### Sprint F

- `V2-4003`
- `V2-5001`
- `V2-5002`
- `V2-5003`

### Sprint G

- `V2-6001`
- `V2-6002`
- `V2-6003`
- `V2-6004`
- `V2-6005`

### Sprint H

- `V2-7001`
- `V2-7002`
- `V2-7101`
- `V2-7102`
- `V2-7103`
- `V2-7104`
- `V2-7105`
- `V2-7106`
- `V2-8001`
- `V2-8002`
- `V2-8003`
- `V2-8004`

## Recommended Start Order Inside Engineering

Parallelizable first-wave work:

- `R-Audio` starts `V2-1001` and `V2-1002`
- `R-DB` starts `V2-0002` and prepares `V2-2001`
- `R-Export` starts `V2-0003` and `V2-3001`
- `F-Record` prepares `V2-1003` around the new finalization contract
- `F-Settings` starts naming and IA preparation for `V2-7101`

Critical-path sequence:

1. `V2-1001`
2. `V2-1002`
3. `V2-1003`
4. `V2-1005`
5. `V2-2001`
6. `V2-2002`
7. `V2-3001`
8. `V2-3003`

## Definition Of Done For v0.2.0

`v0.2.0` is execution-complete when:

- the finalization path is backend-owned and reliable
- library search is FTS-backed and filterable
- markdown export works for single meetings and batches
- auto-export is optional and non-blocking
- import/retranscribe is supported, not hidden as beta
- the `Record` page and `Meeting` workspace are simpler and clearer
- MeetFree Built-in is the primary local summary path and Ollama is the secondary local option
- transcript cleanup and vocabulary controls are functional
- onboarding is truthful
- migration and regression coverage are in place

import { afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingMetadata } from '../../src/services/indexedDBService';
import type { Transcript } from '../../src/types';
import type { RecordingSessionStatus } from '../../src/types';

const sessionId = '11111111-2222-4333-8444-555555555555';
let metadata: MeetingMetadata = { meetingId: `meeting-${sessionId}`, title: 'Recovered meeting', startTime: 1,
  lastUpdated: 2, transcriptCount: 0, savedToSQLite: false };
let empty = true;
let nativeStatus: RecordingSessionStatus | null = null;
let lookupFails = false;
const markSaved = mock(async (_meetingId: string) => {});
const saveMeeting = mock(async (_title: string, _segments: Transcript[], _folder: string | null, _session?: string) => ({ meeting_id: metadata.meetingId }));
const invoke = mock(async (command: string) => {
  if (command === 'get_recording_session_status') {
    if (lookupFails) throw new Error('Native state unavailable');
    return nativeStatus;
  }
  if (command === 'has_audio_checkpoints') return true;
  if (command === 'get_meeting_folder_path') return '/legacy-folder';
  if (command === 'recover_audio_from_checkpoints') return { status: 'success', chunk_count: 1, estimated_duration_seconds: 1, message: 'Mock recovery' };
  if (command === 'cleanup_checkpoints') return null;
  throw new Error(`Unexpected command ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('../../src/services/indexedDBService', () => ({ indexedDBService: {
  getMeetingMetadata: async () => metadata, markMeetingSaved: markSaved,
  getAllMeetings: async () => [metadata],
  getTranscripts: async () => empty ? [] : [{ id: 1, text: 'Legacy words', timestamp: '12:00:00', sequenceId: 1 }],
} }));
mock.module('../../src/services/storageService', () => ({ storageService: { saveMeeting } }));
mock.module('../../src/lib/summary-language-preferences', () => ({ applyPinnedSummaryLanguageToMeeting: async () => {} }));
mock.module('sonner', () => ({ toast: { warning: () => {} } }));
const { useTranscriptRecovery } = await import('../../src/hooks/useTranscriptRecovery');
let hook: ReturnType<typeof useTranscriptRecovery>; let renderer: ReactTestRenderer;
function Harness() { hook = useTranscriptRecovery(); return null; }
beforeEach(() => {
  metadata = { meetingId: `meeting-${sessionId}`, title: 'Recovered meeting', startTime: 1,
    lastUpdated: Date.now() - 5000, transcriptCount: 0, savedToSQLite: false };
  empty = true; nativeStatus = null; lookupFails = false;
  invoke.mockClear(); markSaved.mockClear(); saveMeeting.mockClear();
});
afterEach(() => act(() => renderer?.unmount()));

test('native recovery reuses its session even for empty transcripts and never borrows another recording folder', async () => {
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { await hook.recoverMeeting(metadata.meetingId); });
  expect(saveMeeting.mock.calls.at(-1)).toEqual(['Recovered meeting', [], null, sessionId]);
  expect(invoke.mock.calls).toHaveLength(0);
  expect(markSaved).toHaveBeenCalledTimes(1);
});

test('legacy browser recovery keeps its previous save API behavior', async () => {
  metadata = { ...metadata, meetingId: 'meeting-1234567890' }; empty = false;
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { await hook.recoverMeeting(metadata.meetingId); });
  expect(saveMeeting.mock.calls.at(-1)?.[2]).toBe('/legacy-folder');
  expect(saveMeeting.mock.calls.at(-1)?.[3]).toBeUndefined();
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(['get_meeting_folder_path', 'recover_audio_from_checkpoints', 'cleanup_checkpoints']);
});


test('missed completion events reconcile a successfully saved session before offering recovery', async () => {
  nativeStatus = { status: 'completed', meetingId: metadata.meetingId };
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { await hook.checkForRecoverableTranscripts(); });
  expect(hook.recoverableMeetings).toEqual([]); expect(markSaved.mock.calls).toEqual([[metadata.meetingId]]);
  expect(saveMeeting).not.toHaveBeenCalled();
});

for (const status of ['starting', 'recording', 'paused', 'stopping', 'processing', 'saving'] as const) {
  test(`active ${status} sessions are never offered for recovery`, async () => {
    nativeStatus = { status, meetingId: null };
    await act(async () => { renderer = create(<Harness />); });
    await act(async () => { await hook.checkForRecoverableTranscripts(); });
    expect(hook.recoverableMeetings).toEqual([]); expect(markSaved).not.toHaveBeenCalled();
  });
}

for (const status of ['interrupted', 'error'] as const) {
  test(`${status} sessions remain recoverable even when native transcript recovery already mapped a meeting`, async () => {
    nativeStatus = { status, meetingId: metadata.meetingId }; metadata.folderPath = '/retained-checkpoints';
    await act(async () => { renderer = create(<Harness />); });
    await act(async () => { await hook.checkForRecoverableTranscripts(); });
    expect(hook.recoverableMeetings).toEqual([metadata]); expect(markSaved).not.toHaveBeenCalled();
  });
}

test('unknown native sessions and failed lookups retain recovery data', async () => {
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { await hook.checkForRecoverableTranscripts(); });
  expect(hook.recoverableMeetings).toEqual([metadata]);
  lookupFails = true;
  await act(async () => { await hook.checkForRecoverableTranscripts(); });
  expect(hook.recoverableMeetings).toEqual([metadata]); expect(markSaved).not.toHaveBeenCalled();
});

test('legacy recovery candidates require no native session status lookup', async () => {
  metadata.meetingId = 'meeting-1234567890';
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { await hook.checkForRecoverableTranscripts(); });
  expect(hook.recoverableMeetings).toEqual([metadata]); expect(invoke).not.toHaveBeenCalled();
});

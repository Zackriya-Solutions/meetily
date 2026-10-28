import { afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { RecordingCompletion } from '../../src/types';

let sequence = 0;
const completion = (changes: Partial<RecordingCompletion> = {}): RecordingCompletion => ({
  sessionId: `session-${++sequence}`, meetingId: `meeting-${sequence}`, meetingTitle: 'Daily meeting',
  phase: 'completed', folderPath: '/mock/meeting', transcriptCount: 2, ...changes,
});
let lastResult: RecordingCompletion;
let activeSession: RecordingCompletion | null = null;
let stateLookupFails = false;
const invoke = mock(async (command: string) => {
  if (command === 'get_last_recording_result') return lastResult;
  if (command === 'get_recording_state') {
    if (stateLookupFails) throw new Error('State unavailable');
    return { session: activeSession };
  }
  throw new Error(`Duplicate save or unexpected command: ${command}`);
});
const refetch = mock(async () => {}); const markSaved = mock(async (_session?: string) => {});
const setMeeting = mock(() => {}); const push = mock((_url: string) => {}); const clear = mock(() => {});
const toastSuccess = mock(() => {}); const toastWarning = mock(() => {}); const status = mock((_phase: string, _error?: string) => {});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('next/navigation', () => ({ useRouter: () => ({ push }) }));
mock.module('sonner', () => ({ toast: { success: toastSuccess, warning: toastWarning, error: () => {} } }));
mock.module('../../src/contexts/TranscriptContext', () => ({ useTranscripts: () => ({ clearTranscripts: clear, markMeetingAsSaved: markSaved }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ refetchMeetings: refetch, setCurrentMeeting: setMeeting, setIsMeetingActive: () => {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ RecordingStatus: { ERROR: 'error', COMPLETED: 'completed' }, useRecordingState: () => ({ setStatus: status }) }));
mock.module('../../src/lib/analytics', () => ({ default: { updateMeetingCount: async () => {} } }));
const { useRecordingStop } = await import('../../src/hooks/useRecordingStop');
let hook: ReturnType<typeof useRecordingStop>; let renderer: ReactTestRenderer;
function Harness() { hook = useRecordingStop(() => {}, () => {}); return null; }
beforeEach(async () => {
  activeSession = null; stateLookupFails = false; lastResult = completion();
  for (const fn of [invoke, refetch, markSaved, setMeeting, push, clear, toastSuccess, toastWarning, status]) fn.mockClear();
  await act(async () => { renderer = create(<Harness />); });
});
afterEach(() => act(() => renderer?.unmount()));

function expectPresentedOnce() {
  expect(markSaved).toHaveBeenCalledTimes(1); expect(refetch).toHaveBeenCalledTimes(1);
  expect(toastSuccess).toHaveBeenCalledTimes(1); expect(clear).toHaveBeenCalledTimes(1);
}
function expectNavigatedOnce() {
  expect(push.mock.calls).toEqual([[`/meeting-details?id=${lastResult.meetingId}&source=recording`]]);
}

test('native background completion stays quiet and deduplicates native events', async () => {
  await act(async () => { await hook.handleRecordingCompletion(lastResult, false); await hook.handleRecordingCompletion(lastResult, false); });
  expectPresentedOnce(); expect(push).not.toHaveBeenCalled();
});

test('native background completion first cannot consume subsequent explicit Stop navigation', async () => {
  await act(async () => { await hook.handleRecordingCompletion(lastResult, false); });
  expect(push).not.toHaveBeenCalled();
  await act(async () => { await hook.handleRecordingStop(true, lastResult); });
  expectPresentedOnce(); expectNavigatedOnce();
});

test('explicit Stop first navigates once when native completion follows', async () => {
  await act(async () => { await hook.handleRecordingStop(true, lastResult); await hook.handleRecordingCompletion(lastResult); });
  expectPresentedOnce(); expectNavigatedOnce();
});

test('simultaneous native events and duplicate Stop responses navigate and notify once', async () => {
  await act(async () => { await Promise.all([
    hook.handleRecordingCompletion(lastResult), hook.handleRecordingStop(true), hook.handleRecordingStop(true, lastResult),
  ]); });
  expectPresentedOnce(); expectNavigatedOnce();
  expect(invoke.mock.calls.every(([c]) => ['get_last_recording_result', 'get_recording_state'].includes(c))).toBe(true);
});

test('late explicit Stop preserves a newer recording, its controls, and the current route', async () => {
  activeSession = completion({ phase: 'recording' });
  await act(async () => { await hook.handleRecordingStop(true, lastResult); });
  expect(clear).not.toHaveBeenCalled(); expect(status).not.toHaveBeenCalled(); expect(push).not.toHaveBeenCalled();
  expect(markSaved.mock.calls).toEqual([[lastResult.sessionId]]);
});

test('processing errors still open the saved meeting and retain recovery checkpoints', async () => {
  lastResult = completion({ phase: 'error', error: 'Buffered transcription failed' });
  await act(async () => { await hook.handleRecordingStop(true, lastResult); });
  expect(toastWarning).toHaveBeenCalledTimes(1); expect(markSaved).not.toHaveBeenCalled();
  expect(status.mock.calls.at(-1)).toEqual(['error', 'Buffered transcription failed']); expectNavigatedOnce();
});

test('empty transcript recording still opens its saved meeting', async () => {
  lastResult = completion({ transcriptCount: 0 });
  await act(async () => { await hook.handleRecordingCompletion(lastResult); });
  expectPresentedOnce(); expectNavigatedOnce();
});

test('missing saved result never navigates or marks checkpoints as saved', async () => {
  await act(async () => { await hook.handleRecordingStop(true, null); });
  expect(push).not.toHaveBeenCalled(); expect(markSaved).not.toHaveBeenCalled();
});

test('an unavailable owner check does not navigate away from a possibly active session', async () => {
  stateLookupFails = true;
  await act(async () => { await hook.handleRecordingStop(true, lastResult); });
  expect(push).not.toHaveBeenCalled(); expect(clear).not.toHaveBeenCalled();
});

test('stalled browser cleanup and history refresh cannot delay Stop navigation', async () => {
  markSaved.mockImplementationOnce(() => new Promise<void>(() => {}));
  refetch.mockImplementationOnce(() => new Promise<void>(() => {}));
  await act(async () => { await hook.handleRecordingStop(true, lastResult); });
  expectNavigatedOnce();
});

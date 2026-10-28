import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { NativeRecordingSession, RecordingCompletion } from '../../src/types';

const events = new Map<string, (event: { payload: NativeRecordingSession }) => void>();
let stopped: ((payload: { completion: RecordingCompletion }) => void) | undefined;
const session: NativeRecordingSession = { sessionId: 'session-1', meetingTitle: 'Team meeting', phase: 'paused' };
let backendSession: NativeRecordingSession | null = session;
let legacyRecording = true;
mock.module('../../src/services/recordingService', () => ({ recordingService: new Proxy({}, {
  get: (_, method: string) => method === 'getRecordingState' ? async () => ({ is_recording: legacyRecording, is_paused: legacyRecording, is_active: false,
    recording_duration: 300, active_duration: 299, session: backendSession }) : method === 'onRecordingStopped'
      ? async (callback: typeof stopped) => { stopped = callback; return () => { stopped = undefined; }; }
      : async () => () => {},
}) }));
mock.module('@tauri-apps/api/event', () => ({ listen: async (name: string, cb: (e: { payload: NativeRecordingSession }) => void) => {
  events.set(name, cb); return () => events.delete(name);
} }));
mock.module('sonner', () => ({ toast: { warning: () => {}, error: () => {} } }));
const { RecordingStateProvider, useRecordingState, RecordingStatus } = await import('../../src/contexts/RecordingStateContext');
let state: ReturnType<typeof useRecordingState>; let renderer: ReactTestRenderer;
function Harness() { state = useRecordingState(); return null; }
afterEach(() => { backendSession = session; legacyRecording = true; act(() => renderer?.unmount()); });

test('reload and native events retain owner, title, fixed deadline, pause, and processing state', async () => {
  await act(async () => { renderer = create(<RecordingStateProvider><Harness /></RecordingStateProvider>); });
  expect(state.isPaused).toBe(true);
  const emit = (phase: NativeRecordingSession['phase']) => events.get('recording-session-changed')!({ payload: { ...session, phase } });
  await act(async () => emit('processing'));
  expect(state.isProcessing).toBe(true); expect(state.isRecording).toBe(false);
  expect(state.session?.meetingTitle).toBe('Team meeting');
  await act(async () => emit('saving')); expect(state.isSaving).toBe(true);
  await act(async () => emit('completed')); expect(state.status).toBe(RecordingStatus.COMPLETED); expect(state.isSaving).toBe(false);
});

test('native processing phase takes precedence over the legacy draining flag on reload', async () => {
  backendSession = { ...session, phase: 'processing' };
  await act(async () => { renderer = create(<RecordingStateProvider><Harness /></RecordingStateProvider>); });
  expect(state.isRecording).toBe(false); expect(state.isPaused).toBe(false); expect(state.isActive).toBe(false);
  expect(state.isProcessing).toBe(true); expect(state.session?.sessionId).toBe('session-1');
});

test('an older stop completion cannot clear the current recording owner', async () => {
  await act(async () => { renderer = create(<RecordingStateProvider><Harness /></RecordingStateProvider>); });
  expect(state.session?.sessionId).toBe('session-1');
  await act(async () => stopped!({ completion: { sessionId: 'older', meetingId: 'meeting-older',
    meetingTitle: 'Previous meeting', phase: 'completed', transcriptCount: 0, folderPath: null } }));
  expect(state.isRecording).toBe(true); expect(state.isPaused).toBe(true);
  expect(state.session?.sessionId).toBe('session-1'); expect(state.status).toBe(RecordingStatus.RECORDING);
});


test('stop rejection resync preserves the error when native capture has already ended', async () => {
  await act(async () => { renderer = create(<RecordingStateProvider><Harness /></RecordingStateProvider>); });
  act(() => state.setStatus(RecordingStatus.ERROR, 'Disk is full'));
  backendSession = null; legacyRecording = false;
  await act(async () => { await state.syncWithBackend(); });
  expect(state.isRecording).toBe(false); expect(state.session).toBeUndefined();
  expect(state.status).toBe(RecordingStatus.ERROR); expect(state.statusMessage).toBe('Disk is full');
});

test('stop rejection resync restores a still-live recording and clears its transient error status', async () => {
  await act(async () => { renderer = create(<RecordingStateProvider><Harness /></RecordingStateProvider>); });
  act(() => state.setStatus(RecordingStatus.ERROR, 'Stop request failed'));
  await act(async () => { await state.syncWithBackend(); });
  expect(state.isRecording).toBe(true); expect(state.isPaused).toBe(true);
  expect(state.status).toBe(RecordingStatus.RECORDING); expect(state.statusMessage).toBeUndefined();
});

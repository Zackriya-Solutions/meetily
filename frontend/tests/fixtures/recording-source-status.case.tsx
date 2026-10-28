import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { RecordingSourceStatus } from '../../src/lib/recording-source';

const events = new Map<string, () => void>();
const options = { source: { kind: 'application' as const, application: { name: 'Zoom', bundleId: 'test.zoom', bundlePath: '/Applications/Zoom.app' } }, microphoneEnabled: false };
let source: RecordingSourceStatus = { sessionId: 'session-a', options, state: 'waiting', message: 'Waiting for Zoom' };
const getState = () => ({ is_recording: true, is_paused: false, is_active: true, recording_duration: 3, active_duration: 3, source_status: source });
let readState = async () => getState();
mock.module('../../src/services/recordingService', () => ({ recordingService: new Proxy({}, {
  get: (_, method: string) => method === 'getRecordingState' ? () => readState() : async () => () => {},
}) }));
mock.module('@tauri-apps/api/event', () => ({ listen: async (event: string, callback: () => void) => {
  events.set(event, callback); return () => events.delete(event);
} }));
const { RecordingStateProvider, useRecordingState } = await import('../../src/contexts/RecordingStateContext');
let observed: ReturnType<typeof useRecordingState>;
function Status() { observed = useRecordingState(); return <output>{observed.sourceStatus?.message}</output>; }
let renderer: ReactTestRenderer;
afterEach(() => { if (renderer) act(() => renderer.unmount()); });

test('reload restores the backend source and waiting status; events read the current session', async () => {
  await act(async () => { renderer = create(<RecordingStateProvider><Status /></RecordingStateProvider>); });
  expect(observed.isRecording).toBe(true);
  expect(observed.sourceStatus?.options.microphoneEnabled).toBe(false);
  expect(observed.sourceStatus?.message).toBe('Waiting for Zoom');
  source = { ...source, sessionId: 'session-b', state: 'capturing', message: null };
  await act(async () => events.get('recording-source-changed')!());
  expect(observed.sourceStatus?.sessionId).toBe('session-b');
  expect(observed.sourceStatus?.state).toBe('capturing');
});

test('an older in-flight read cannot replace a newer session snapshot', async () => {
  let finishOld!: (state: ReturnType<typeof getState>) => void;
  readState = () => new Promise(resolve => { finishOld = resolve; });
  await act(async () => { renderer = create(<RecordingStateProvider><Status /></RecordingStateProvider>); });
  const oldState = getState();
  source = { ...source, sessionId: 'new-session', state: 'waiting', message: 'Waiting for Zoom' };
  readState = async () => getState();
  await act(async () => events.get('recording-source-changed')!());
  await act(async () => finishOld(oldState));
  expect(observed.sourceStatus?.sessionId).toBe('new-session');
});

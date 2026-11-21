import { afterAll, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvents = { ...await import('@tauri-apps/api/event') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvents);
});
let backendActive = false;
const handlers = new Map<string, (event: { payload: any }) => void>();
mock.module('@tauri-apps/api/core', () => ({ invoke: async () => ({
  is_recording: backendActive, is_paused: false, is_active: backendActive,
  recording_duration: null, active_duration: null,
}) }));
mock.module('@tauri-apps/api/event', () => ({ listen: async (name: string, handler: any) => {
  handlers.set(name, handler);
  return () => { handlers.delete(name); };
} }));
const { RecordingStateProvider, useRecordingState, RecordingStatus } = await import('../../src/contexts/RecordingStateContext');
for (const recovered of [false, true]) {
test(`admitted shutdown preserves later processing, recovered=${recovered}`, async () => {
  backendActive = recovered;
  let state: ReturnType<typeof useRecordingState>;
  function Status() { state = useRecordingState(); return <span>{state.status}</span>; }
  let view: ReturnType<typeof create>;
  try {
    await act(async () => { view = create(<RecordingStateProvider><Status /></RecordingStateProvider>); });
    if (!recovered) await act(async () => { handlers.get('recording-started')!({ payload: {} }); });
    const initialStatus = recovered ? RecordingStatus.IDLE : RecordingStatus.RECORDING;
    expect(state!.status).toBe(initialStatus);
    const progress = handlers.get('recording-shutdown-progress');
    expect(progress).toBeDefined();
    await act(async () => { progress!({ payload: { stage: 'processing_transcripts' } }); });
    expect(state!.status).toBe(initialStatus);
    await act(async () => { progress!({ payload: { stage: 'stopping_audio' } }); });
    expect(state!.isStopping).toBe(true);
    await act(async () => { progress!({ payload: { stage: 'stopping_audio' } }); });
    expect(state!.isStopping).toBe(true);
    await act(async () => { state!.setStatus(RecordingStatus.PROCESSING_TRANSCRIPTS); });
    await act(async () => { progress!({ payload: { stage: 'stopping_audio' } }); });
    expect(state!.status).toBe(RecordingStatus.PROCESSING_TRANSCRIPTS);
  } finally {
    await act(async () => { view!.unmount(); });
  }
  expect(handlers.has('recording-shutdown-progress')).toBe(false);
});
}

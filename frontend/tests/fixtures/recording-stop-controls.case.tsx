import { afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { ReactNode } from 'react';
import type { RecordingCompletion } from '../../src/types';

const completion: RecordingCompletion = { sessionId: 'manual-stop', meetingId: 'meeting-manual-stop',
  meetingTitle: 'Test', phase: 'completed', transcriptCount: 0, folderPath: null };
let finishStop: (result: RecordingCompletion | null) => void;
let rejectStop: (error: unknown) => void;
const invoke = mock(async (command: string, _args?: { args: { session_id: string } }) => {
  if (command === 'is_recording') return true;
  if (command === 'stop_recording') return new Promise<RecordingCompletion | null>((resolve, reject) => { finishStop = resolve; rejectStop = reject; });
  throw new Error(`Unexpected invoke: ${command}`);
});
const stopped = mock(async (_callApi?: boolean, _result?: RecordingCompletion | null) => {});
const status = mock((_phase: string, _message?: string) => {});
const sync = mock(async () => {});
const toastError = mock((_title: string, _options: { description: string }) => {});
let paused = false;
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
mock.module('sonner', () => ({ toast: { error: toastError } }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  RecordingStatus: { STOPPING: 'stopping', ERROR: 'error' },
  useRecordingState: () => ({ session: { sessionId: completion.sessionId }, isPaused: paused,
    isStartingRecording: false, setStatus: status, syncWithBackend: sync }),
}));
mock.module('../../src/lib/analytics', () => ({ default: {
  trackButtonClick: () => {}, trackTranscriptionSuccess: () => {},
} }));
// Tooltips are decoration; their browser event plumbing is outside this control test.
const Wrapper = ({ children }: { children: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/tooltip', () => ({
  TooltipProvider: Wrapper, Tooltip: Wrapper, TooltipTrigger: Wrapper, TooltipContent: () => null,
}));
const { RecordingControls } = await import('../../src/components/RecordingControls');
let renderer: ReactTestRenderer;
async function mount() {
  await act(async () => { renderer = create(<RecordingControls isRecording barHeights={[]}
    onRecordingStop={stopped} onRecordingStart={() => {}} onTranscriptReceived={() => {}}
    isRecordingDisabled={false} isParentProcessing={false} />); });
}
function stopButton() {
  // Stop is the recording control containing the square icon.
  return renderer.root.findAllByType('button').find(button =>
    button.findAll(node => node.type === 'svg' && String(node.props.className).includes('lucide-square')).length > 0)!;
}
beforeEach(() => { paused = false; for (const fn of [invoke, stopped, status, sync, toastError]) fn.mockClear(); });
afterEach(() => act(() => renderer?.unmount()));

test('Stop changes status immediately and rejects same-frame duplicate clicks while native finalization is pending', async () => {
  await mount(); const click = stopButton().props.onClick;
  act(() => { click(); click(); });
  expect(invoke.mock.calls.filter(([command]) => command === 'stop_recording')).toHaveLength(1);
  expect(status.mock.calls[0]?.[0]).toBe('stopping');
  expect(JSON.stringify(renderer.toJSON())).toContain('Processing recording...');
  await act(async () => { finishStop(completion); });
  expect(stopped.mock.calls).toEqual([[true, completion]]);
});

test('a paused scheduled recording uses the same stop and completion path', async () => {
  paused = true; await mount();
  act(() => { stopButton().props.onClick(); });
  expect(invoke.mock.calls.find(([command]) => command === 'stop_recording')).toEqual([
    'stop_recording', { args: { session_id: completion.sessionId } },
  ]);
  await act(async () => { finishStop(completion); });
  expect(stopped.mock.calls).toEqual([[true, completion]]);
});

test('native stop failure displays its error, resynchronizes state, and permits retry', async () => {
  await mount(); act(() => { stopButton().props.onClick(); });
  await act(async () => { rejectStop('Disk is full'); });
  expect(toastError.mock.calls[0]?.[1].description).toBe('Disk is full');
  expect(status.mock.calls.at(-1)).toEqual(['error', 'Disk is full']); expect(sync).toHaveBeenCalledTimes(1);
  expect(stopped.mock.calls).toEqual([[false]]);
  act(() => { stopButton().props.onClick(); });
  expect(invoke.mock.calls.filter(([command]) => command === 'stop_recording')).toHaveLength(2);
  await act(async () => { finishStop(completion); });
  expect(stopped.mock.calls.at(-1)).toEqual([true, completion]);
});

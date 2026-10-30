import React from 'react';
import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

let outcome = 'stopped';
let releaseStop: (() => void) | undefined;
const invoke = mock(async (command: string) => {
  if (command !== 'stop_recording') return true;
  if (outcome === 'pending') {
    await new Promise<void>(resolve => { releaseStop = resolve; });
    return 'stopped';
  }
  if (outcome === 'failure') throw new Error('Save failed');
  return outcome;
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/path', () => ({ appDataDir: async () => '/tmp' }));
mock.module('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  useRecordingState: () => ({ isPaused: false, isStartingRecording: false }),
}));
mock.module('../../src/lib/analytics', () => ({ default: {
  trackButtonClick: () => {}, trackTranscriptionSuccess: () => {},
} }));
const Container = ({ children }: any) => <div>{children}</div>;
mock.module('../../src/components/ui/tooltip', () => ({
  Tooltip: Container, TooltipTrigger: Container, TooltipContent: Container, TooltipProvider: Container,
}));
const { RecordingControls } = await import('../../src/components/RecordingControls');
let renderer: ReactTestRenderer | undefined;
afterEach(async () => { if (renderer) await act(async () => renderer!.unmount()); });

for (const result of ['stopped', 'already_stopping', 'not_active']) {
  test(`Stop outcome ${result} only processes the owning shutdown`, async () => {
    outcome = result;
    const postprocess = mock(() => {});
    const initiate = mock(() => {});
    await act(async () => {
      renderer = create(<RecordingControls isRecording barHeights={[]} onRecordingStop={postprocess}
        onRecordingStart={() => {}} onTranscriptReceived={() => {}}
        isRecordingDisabled={false} isParentProcessing={false} onStopInitiated={initiate} />);
    });
    await act(async () => {
      renderer!.root.findAllByType('button')[1].props.onClick();
    });
    if (result === 'stopped') expect(postprocess).toHaveBeenCalledWith(true);
    else expect(postprocess).not.toHaveBeenCalled();
    expect(initiate).toHaveBeenCalledTimes(result === 'stopped' ? 1 : 0);
    expect(JSON.stringify(renderer!.toJSON())).not.toContain('Processing recording...');
  });
}

test('owner completion waits for the native stop and preserves errors', async () => {
  for (const result of ['pending', 'failure']) {
    outcome = result;
    releaseStop = undefined;
    const postprocess = mock(() => {});
    await act(async () => {
      renderer = create(<RecordingControls isRecording barHeights={[]} onRecordingStop={postprocess}
        onRecordingStart={() => {}} onTranscriptReceived={() => {}}
        isRecordingDisabled={false} isParentProcessing={false} />);
    });
    await act(async () => { renderer!.root.findAllByType('button')[1].props.onClick(); });
    if (result === 'pending') {
      expect(postprocess).not.toHaveBeenCalled();
      expect(releaseStop).toBeDefined();
      await act(async () => releaseStop!());
      expect(postprocess).toHaveBeenCalledWith(true);
    } else expect(postprocess).toHaveBeenCalledWith(false);
    await act(async () => renderer!.unmount());
    renderer = undefined;
  }
});

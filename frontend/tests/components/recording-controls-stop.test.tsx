import React from 'react';
import { afterAll, afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
const originals = await Promise.all(['@tauri-apps/api/core', '@tauri-apps/api/path', '@tauri-apps/api/event', '../../src/contexts/RecordingStateContext', '../../src/lib/analytics', '../../src/components/ui/tooltip'].map(async path =>
  [path, { ...await import(path) }] as const));
afterAll(() => { for (const [path, exports] of originals) mock.module(path, () => exports); });


let outcome = 'stopped';
let globalStopping = false;
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
  useRecordingState: () => ({ isPaused: false, isStartingRecording: false, isStopping: globalStopping }),
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

for (const message of [
  'Your saved audio selection needs updating. Choose a device in Recording Settings.',
  'Your saved microphone selection needs updating. Choose a device in Recording Settings.',
  'Your saved system-audio selection needs updating. Choose a device in Recording Settings.',
]) {
  test(`Start preserves migration repair guidance: ${message}`, async () => {
    await act(async () => {
      renderer = create(<RecordingControls isRecording={false} barHeights={[]} onRecordingStop={() => {}}
        onRecordingStart={() => { throw new Error(message); }} onTranscriptReceived={() => {}}
        isRecordingDisabled={false} isParentProcessing={false} />);
    });
    await act(async () => { renderer!.root.findAllByType('button')[0].props.onClick(); });
    expect(JSON.stringify(renderer!.toJSON())).toContain(message);
  });
}

test('global stopping keeps the processing indicator after a losing Stop', async () => {
  outcome = 'already_stopping';
  globalStopping = false;
  const postprocess = mock(() => {});
  const props = { isRecording: true, barHeights: [], onRecordingStop: postprocess,
    onRecordingStart: () => {}, onTranscriptReceived: () => {},
    isRecordingDisabled: false, isParentProcessing: false };
  await act(async () => { renderer = create(<RecordingControls {...props} />); });
  await act(async () => { renderer!.root.findAllByType('button')[1].props.onClick(); });
  globalStopping = true;
  await act(async () => { renderer!.update(<RecordingControls {...props} />); });
  expect(JSON.stringify(renderer!.toJSON())).toContain('Processing recording...');
  expect(postprocess).not.toHaveBeenCalled();
  globalStopping = false;
});

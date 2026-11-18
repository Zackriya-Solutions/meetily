import { afterAll, beforeAll, describe, expect, mock, test } from 'bun:test';
import type React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

// RecordingControls' listeners must survive onRecordingStop changing identity
// (it does at recording start) without leaking: every listener registered must be
// unlistened, even one whose listen() was still pending when the effect cleaned
// up. A leaked `transcription-error` listener calls a stale onRecordingStop again.
const originals = {
  recordingState: null as unknown as Record<string, unknown>,
  event: { ...await import('@tauri-apps/api/event') },
  core: { ...await import('@tauri-apps/api/core') },
  path: { ...await import('@tauri-apps/api/path') },
  analytics: { ...await import('../../src/lib/analytics') },
  tooltip: { ...await import('../../src/components/ui/tooltip') },
};
afterAll(() => {
  if (originals.recordingState) mock.module('../../src/contexts/RecordingStateContext', () => originals.recordingState);
  mock.module('@tauri-apps/api/event', () => originals.event);
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('@tauri-apps/api/path', () => originals.path);
  mock.module('../../src/lib/analytics', () => originals.analytics);
  mock.module('../../src/components/ui/tooltip', () => originals.tooltip);
  if (!hadWindow) delete (globalThis as any).window;
  if (!hadSessionStorage) delete (globalThis as any).sessionStorage;
});
const hadWindow = 'window' in globalThis;
const hadSessionStorage = 'sessionStorage' in globalThis;
(globalThis as any).window ??= { addEventListener() {}, removeEventListener() {} };
(globalThis as any).sessionStorage ??= { getItem: () => null, removeItem() {} };

type Registration = {
  name: string;
  handler: (event: { payload: unknown }) => void;
  unlistened: boolean;
  resolve: () => void;
};
const registrations: Registration[] = [];
const pending: Registration[] = [];
mock.module('@tauri-apps/api/event', () => ({
  ...originals.event,
  listen: (name: string, handler: Registration['handler']) =>
    new Promise<() => void>(resolveListen => {
      const registration: Registration = {
        name,
        handler,
        unlistened: false,
        resolve: () => resolveListen(() => { registration.unlistened = true; }),
      };
      registrations.push(registration);
      pending.push(registration);
    }),
}));
const invokeCalls: string[] = [];
mock.module('@tauri-apps/api/core', () => ({
  ...originals.core,
  // stop_recording answers with a StopOutcome; only 'stopped' runs post-processing.
  invoke: async (command: string) => { invokeCalls.push(command); return command === 'stop_recording' ? 'stopped' : false; },
}));
mock.module('@tauri-apps/api/path', () => ({ ...originals.path, appDataDir: async () => '/data' }));
mock.module('../../src/lib/analytics', () => ({ default: { trackTranscriptionError() {}, trackTranscriptionSuccess() {}, trackButtonClick() {} } }));
// Radix tooltips need a DOM; this test only cares about the listeners.
const Passthrough = ({ children }: { children?: React.ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/tooltip', () => ({
  Tooltip: Passthrough, TooltipContent: Passthrough, TooltipProvider: Passthrough, TooltipTrigger: Passthrough,
}));

let RecordingControls: React.FC<any>;
beforeAll(async () => {
  originals.recordingState = { ...await import('../../src/contexts/RecordingStateContext') };
  mock.module('../../src/contexts/RecordingStateContext', () => ({
    ...originals.recordingState,
    useRecordingState: () => ({ isPaused: false, isStartingRecording: false }),
  }));
  ({ RecordingControls } = await import('../../src/components/RecordingControls'));
});

async function resolveAllPending() {
  while (pending.length) {
    await act(async () => {
      for (const r of pending.splice(0)) r.resolve();
    });
  }
}

describe('RecordingControls event listeners', () => {
  test('a re-render while listen() is pending leaks no listener', async () => {
    const props = {
      isRecording: true,
      onRecordingStart: async () => {},
      onTranscriptReceived: () => {},
      isRecordingDisabled: false,
      isParentProcessing: false,
    };
    const firstStop = mock((_callApi?: boolean) => {});
    const secondStop = mock((_callApi?: boolean) => {});
    let renderer: ReactTestRenderer | undefined;
    await act(async () => {
      renderer = create(<RecordingControls {...props} onRecordingStop={firstStop} />);
    });
    await act(async () => renderer!.update(<RecordingControls {...props} onRecordingStop={secondStop} />));
    await resolveAllPending();

    await act(async () => {
      for (const r of registrations.filter(r => r.name === 'transcription-error' && !r.unlistened)) {
        r.handler({ payload: { error: 'boom', phase: 'active' } });
      }
    });
    expect(firstStop).toHaveBeenCalledTimes(0);
    expect(secondStop.mock.calls).toEqual([[true]]);

    await act(async () => renderer!.unmount());
    await resolveAllPending();
    expect(registrations.length).toBeGreaterThan(0);
    expect(registrations.filter(r => !r.unlistened).map(r => r.name)).toEqual([]);
  });
});

describe('RecordingControls transcription-error', () => {
  const props = {
    isRecording: true,
    onRecordingStart: async () => {},
    onTranscriptReceived: () => {},
    isRecordingDisabled: false,
    isParentProcessing: false,
  };

  async function renderAndEmit(payload: Record<string, unknown>) {
    const onRecordingStop = mock((_callApi?: boolean) => {});
    let renderer: ReactTestRenderer | undefined;
    await act(async () => {
      renderer = create(<RecordingControls {...props} onRecordingStop={onRecordingStop} />);
    });
    await resolveAllPending();
    invokeCalls.length = 0;
    await act(async () => {
      for (const r of registrations.filter(r => r.name === 'transcription-error' && !r.unlistened)) {
        r.handler({ payload });
      }
    });
    await act(async () => renderer!.unmount());
    await resolveAllPending();
    return onRecordingStop;
  }

  test('a per-chunk error leaves the recording running', async () => {
    // Shape of worker.rs chunk_error_payload: a toast, no phase.
    const onRecordingStop = await renderAndEmit({
      error: 'Transcription engine failed: HTTP 503',
      userMessage: 'Transcription failed: Transcription engine failed: HTTP 503',
      actionable: false,
    });
    expect(onRecordingStop).toHaveBeenCalledTimes(0);
    expect(invokeCalls).toEqual([]);
  });

  test('a fatal error stops the backend recording before post-processing', async () => {
    const onRecordingStop = await renderAndEmit({
      error: 'engine init failed',
      userMessage: 'Recording failed: Unable to initialize speech recognition.',
      actionable: true,
      phase: 'active',
    });
    expect(invokeCalls).toEqual(['stop_recording']);
    expect(onRecordingStop.mock.calls).toEqual([[true]]);
  });
});

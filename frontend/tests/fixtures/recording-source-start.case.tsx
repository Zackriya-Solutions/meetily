import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originals = (await Promise.all([
  import('../../src/contexts/TranscriptContext'), import('../../src/components/Sidebar/SidebarProvider'),
  import('../../src/contexts/ConfigContext'), import('../../src/contexts/RecordingStateContext'),
  import('../../src/services/recordingService'), import('../../src/lib/analytics'),
  import('../../src/lib/recordingNotification'), import('@tauri-apps/api/core'),
])).map(module => ({ ...module }));
const paths = ['../../src/contexts/TranscriptContext', '../../src/components/Sidebar/SidebarProvider',
  '../../src/contexts/ConfigContext', '../../src/contexts/RecordingStateContext',
  '../../src/services/recordingService', '../../src/lib/analytics', '../../src/lib/recordingNotification', '@tauri-apps/api/core'];
afterAll(() => { paths.forEach((path, i) => mock.module(path, () => originals[i])); });

const noop = () => {};
const options = { source: { kind: 'application', application: { bundleId: 'test.zoom', bundlePath: '/Applications/Zoom.app', name: 'Zoom' } }, microphoneEnabled: false };
const devices = { micDevice: null, systemDevice: null, sourceOptions: options };
const start = mock(async (..._args: unknown[]) => {});
mock.module('../../src/contexts/TranscriptContext', () => ({ useTranscripts: () => ({ clearTranscripts: noop, setMeetingTitle: noop }) }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({ useSidebar: () => ({ setIsMeetingActive: noop }) }));
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => ({ selectedDevices: devices }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ ...originals[3], useRecordingState: () => ({ setStatus: noop }) }));
mock.module('../../src/services/recordingService', () => ({ recordingService: { startRecordingWithDevices: start } }));
mock.module('../../src/lib/analytics', () => ({ default: { trackButtonClick: noop } }));
mock.module('../../src/lib/recordingNotification', () => ({ showRecordingNotification: async () => {} }));
mock.module('@tauri-apps/api/core', () => ({ invoke: async (command: string) => command === 'api_get_transcript_config' ? { provider: 'localWhisper' } : true }));

const { useRecordingStart } = await import('../../src/hooks/useRecordingStart');
let hook: ReturnType<typeof useRecordingStart>;
function Harness() { hook = useRecordingStart(false, noop); return null; }
let renderer: ReactTestRenderer;
const listeners = new Map<string, () => Promise<void>>();
const storage = new Map<string, string>();
const windowDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
const storageDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
beforeEach(() => {
  listeners.clear(); storage.clear(); start.mockClear();
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    addEventListener: (name: string, cb: () => Promise<void>) => listeners.set(name, cb),
    removeEventListener: (name: string) => listeners.delete(name),
  } });
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: {
    getItem: (name: string) => storage.get(name), removeItem: (name: string) => storage.delete(name),
  } });
});
afterEach(() => {
  if (renderer) act(() => renderer.unmount());
  for (const [name, descriptor] of [['window', windowDescriptor], ['sessionStorage', storageDescriptor]] as const) {
    if (descriptor) Object.defineProperty(globalThis, name, descriptor); else Reflect.deleteProperty(globalThis, name);
  }
});
function checkSource() {
  expect(start).toHaveBeenCalledTimes(1);
  const [mic, system, title, source] = start.mock.calls[0];
  expect(mic).toBeNull(); expect(system).toBeNull();
  expect(typeof title).toBe('string'); expect(source).toEqual(options);
}
describe('recording source through all start routes', () => {
  test('manual start retains app selection even with two default hardware devices', async () => {
    await act(async () => { renderer = create(<Harness />); });
    await act(async () => hook.handleRecordingStart());
    checkSource();
  });
  test('navigation auto-start retains microphone off and app selection', async () => {
    storage.set('autoStartRecording', 'true');
    await act(async () => { renderer = create(<Harness />); });
    checkSource();
  });
  test('direct sidebar start retains microphone off and app selection', async () => {
    await act(async () => { renderer = create(<Harness />); });
    const callback = [...listeners.values()][0];
    expect(callback).toBeDefined();
    await act(async () => callback());
    checkSource();
  });
});

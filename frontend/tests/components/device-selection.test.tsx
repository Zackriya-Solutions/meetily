import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
mock.module('@tauri-apps/api/core', () => ({ invoke: mock(async () => []) }));
mock.module('@tauri-apps/api/event', () => ({ listen: mock(async () => () => {}) }));
mock.module('../../src/lib/analytics', () => ({ default: { track: async () => {} } }));
mock.module('../../src/components/ui/select', () => ({
  Select: ({ children }: any) => <div>{children}</div>,
  SelectContent: ({ children }: any) => <div>{children}</div>,
  SelectItem: ({ children }: any) => <div>{children}</div>,
  SelectTrigger: ({ children }: any) => <div>{children}</div>,
  SelectValue: () => <span />,
}));
const { DeviceSelection } = await import('../../src/components/DeviceSelection');
let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  Reflect.deleteProperty(globalThis, 'window');
});
test('recording device settings render after loading without OS plugin internals', async () => {
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
  await act(async () => {
    renderer = create(<DeviceSelection selectedDevices={{ micDevice: null, systemDevice: null }} onDeviceChange={() => {}} />);
  });
  expect(JSON.stringify(renderer!.toJSON())).toContain('System Audio');
});

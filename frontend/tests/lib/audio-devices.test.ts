import { describe, expect, mock, test } from 'bun:test';
mock.module('@tauri-apps/api/core', () => ({ invoke: mock(async () => []) }));
const { encodeDeviceOption, decodeDeviceOption, deviceLabel, isDeviceAvailable } = await import('../../src/lib/audioDevices');
describe('audio device identity', () => {
  test('default and literal IDs cannot collide', () => {
    for (const id of [null, 'default:', 'device:mic', 'USB Mic (input)']) {
      expect(decodeDeviceOption(encodeDeviceOption(id))).toBe(id);
    }
  });
  test('matching ignores duplicate labels and respects role', () => {
    const devices = [
      { name: 'source.a', label: 'Headset', device_type: 'Input' as const },
      { name: 'sink.a', label: 'Headset', device_type: 'Output' as const },
    ];
    expect(isDeviceAvailable(devices, 'Input', 'Headset')).toBe(false);
    expect(isDeviceAvailable(devices, 'Input', 'sink.a')).toBe(false);
    expect(isDeviceAvailable(devices, 'Input', 'source.a')).toBe(true);
    expect(deviceLabel(devices[0])).toBe('Headset');
  });
});

import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { useState } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { RecordingSourceOptions } from '../../src/lib/recording-source';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalSwitch = { ...await import('../../src/components/ui/switch') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('../../src/components/ui/switch', () => originalSwitch);
});
const zoom = { bundleId: 'test.zoom', bundlePath: '/Applications/Zoom.app', name: 'Zoom', audioActive: false };
let apps = [zoom];
let supported = true;
let calls: string[] = [];
mock.module('@tauri-apps/api/core', () => ({ invoke: async (command: string) => {
  calls.push(command);
  if (command === 'get_audio_capture_capabilities') return { applicationAudio: supported, reason: supported ? null : 'macOS required' };
  if (command === 'list_audio_applications') return apps;
  throw new Error(command);
} }));
mock.module('../../src/components/ui/switch', () => ({ Switch: (props: { checked: boolean; disabled: boolean; onCheckedChange: (checked: boolean) => void; id: string }) =>
  <input type="checkbox" {...props} onChange={() => props.onCheckedChange(!props.checked)} /> }));
const { ApplicationAudioSelector } = await import('../../src/components/ApplicationAudioSelector');
let renderer: ReactTestRenderer;
let selected: RecordingSourceOptions;
function Harness({ disabled = false }: { disabled?: boolean }) {
  const [options, setOptions] = useState<RecordingSourceOptions>({ source: { kind: 'application', application: zoom }, microphoneEnabled: true });
  selected = options;
  return <ApplicationAudioSelector options={options} onChange={setOptions} disabled={disabled} />;
}
beforeEach(() => { apps = [zoom]; supported = true; calls = []; });
afterEach(() => { if (renderer) act(() => renderer.unmount()); });

describe('application audio selection', () => {
  test('lists idle apps, searches, and changes mic independently', async () => {
    await act(async () => { renderer = create(<Harness />); });
    expect(JSON.stringify(renderer.toJSON())).toContain('Idle');
    await act(async () => renderer.root.findByProps({ id: 'include-microphone', type: 'checkbox' }).props.onChange());
    expect(selected.microphoneEnabled).toBe(false);
    expect(selected.source.kind).toBe('application');
    await act(async () => renderer.root.findByProps({ type: 'search' }).props.onChange({ target: { value: 'missing' } }));
    expect(JSON.stringify(renderer.toJSON())).toContain('No matching running apps');
    expect(selected.source.kind).toBe('application');
  });
  test('missing app remains selected instead of falling back to system audio', async () => {
    apps = [];
    await act(async () => { renderer = create(<Harness />); });
    expect(JSON.stringify(renderer.toJSON())).toContain('unavailable');
    expect(selected.source.kind).toBe('application');
  });
  test('locks controls during a session and does not enumerate apps', async () => {
    await act(async () => { renderer = create(<Harness disabled />); });
    for (const element of renderer.root.findAllByType('select')) expect(element.props.disabled).toBe(true);
    expect(renderer.root.findByProps({ type: 'checkbox' }).props.disabled).toBe(true);
    expect(calls).not.toContain('list_audio_applications');
  });
  test('unsupported platform preserves the saved selection and disables application capture', async () => {
    supported = false;
    await act(async () => { renderer = create(<Harness />); });
    expect(renderer.root.findByProps({ 'aria-label': 'Application to record' }).props.disabled).toBe(true);
    expect(selected.source.kind).toBe('application');
    expect(calls).not.toContain('list_audio_applications');
  });
});

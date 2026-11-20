import React from 'react';
import { afterAll, afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const mocked = [
  '@tauri-apps/api/core',
  '@tauri-apps/api/event',
  'sonner',
  '../../src/components/Sidebar/SidebarProvider',
  '../../src/components/ConfirmationModel/confirmation-modal',
];
const originals = await Promise.all(mocked.map(async path => [path, { ...await import(path) }] as const));
afterAll(() => { for (const [path, exports] of originals) mock.module(path, () => exports); });

const item = (meeting_id: string, queued: boolean) => ({
  meeting_id, title: meeting_id, stage: 'transcribe', created_at: '2026-11-19T00:00:00Z',
  attempts: 0, last_error: null, suppressed: false, eligible: true, queued,
});
let status: Record<string, unknown>;
const invoke = mock(async (command: string) => (command === 'pipeline_get_status' ? status : undefined));
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
mock.module('sonner', () => ({ toast: { success() {}, error() {} } }));
mock.module('../../src/components/Sidebar/SidebarProvider', () => ({
  useSidebar: () => ({ refetchMeetings: async () => {} }),
}));
mock.module('../../src/components/ConfirmationModel/confirmation-modal', () => ({
  ConfirmationModal: () => null,
}));
const { PendingWorkPanel } = await import('../../src/components/shared/PendingWorkPanel');

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  invoke.mockClear();
});

const render = async (next: Record<string, unknown>) => {
  status = { detail: null, enabled: true, last_scan_at: null, ...next };
  await act(async () => { renderer = create(<PendingWorkPanel />); });
};
const stopButton = () =>
  renderer!.root.findAllByType('button').find(button => String(button.props.title ?? '').startsWith('Cancel the meeting in progress'));

// The state in the report: paused, one meeting transcribing, two more queued
// with Process now. Pause was already on, so nothing could halt them.
test('a paused pipeline with queued meetings offers Stop, which calls pipeline_stop', async () => {
  await render({
    state: 'paused',
    current: { meeting_id: 'a', title: 'a', stage: 'transcribe', started_at: '2026-11-19T00:00:00Z' },
    pending: [item('a', true), item('b', true), item('c', true)],
  });
  const stop = stopButton();
  expect(stop).toBeDefined();
  await act(async () => { stop!.props.onClick(); });
  expect(invoke.mock.calls.map(call => call[0])).toContain('pipeline_stop');
});

test('Stop is offered for queued meetings even before one starts', async () => {
  await render({ state: 'paused', current: null, pending: [item('a', true), item('b', false)] });
  expect(stopButton()).toBeDefined();
});

test('nothing running and nothing queued leaves only Pause', async () => {
  await render({ state: 'running', current: null, pending: [item('a', false)] });
  expect(stopButton()).toBeUndefined();
});

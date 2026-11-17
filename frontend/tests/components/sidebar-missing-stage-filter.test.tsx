import { afterAll, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalEvent = { ...await import('@tauri-apps/api/event') };
const originalNavigation = { ...await import('next/navigation') };
const originalAnalytics = { ...await import('../../src/lib/analytics') };
const originalRecordingState = { ...await import('../../src/contexts/RecordingStateContext') };
afterAll(() => {
  mock.module('../../src/contexts/RecordingStateContext', () => originalRecordingState);
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('@tauri-apps/api/event', () => originalEvent);
  mock.module('next/navigation', () => originalNavigation);
  mock.module('../../src/lib/analytics', () => originalAnalytics);
});

mock.module('next/navigation', () => ({ usePathname: () => '/meeting-details', useRouter: () => ({ push() {} }) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({ useRecordingState: () => ({ isRecording: false }) }));
mock.module('../../src/lib/analytics', () => ({ default: { trackBackendConnection() {}, trackPageView() {} } }));

const handlers = new Map<string, (event: { payload: unknown }) => void>();
mock.module('@tauri-apps/api/event', () => ({
  ...originalEvent,
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(name, handler);
    return () => { if (handlers.get(name) === handler) handlers.delete(name); };
  },
}));

// `recap` has no transcript and no recording to make one from, as a meeting made from a Teams recap.
let backendMeetings = [
  { id: 'recorded', title: 'Recorded', transcribed: false, transcript_needed: true, summarized: false, obsidian_exported: false },
  { id: 'transcribed', title: 'Transcribed', transcribed: true, transcript_needed: false, summarized: false, obsidian_exported: false },
  { id: 'done', title: 'Done', transcribed: true, transcript_needed: false, summarized: true, obsidian_exported: true },
  { id: 'recap', title: 'Recap', transcribed: false, transcript_needed: false, summarized: true, obsidian_exported: true },
];
mock.module('@tauri-apps/api/core', () => ({
  ...originalCore,
  invoke: async (command: string) => {
    if (command === 'api_get_meetings') {
      return backendMeetings;
    }
    throw new Error(`Unexpected command: ${command}`);
  },
}));
const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');
const { missingStage } = await import('../../src/components/Sidebar/meetingStages');

let context!: ReturnType<typeof useSidebar>;
function Probe() {
  context = useSidebar();
  return null;
}
const flush = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
const shown = (stage: Parameters<typeof missingStage>[1]) =>
  missingStage(context.sidebarItems, stage)[0].children!.map(item => item.id);

describe('sidebar missing-stage filter', () => {
  test('shows meetings missing each stage and refreshes on meetings-changed', async () => {
    let renderer: ReturnType<typeof create> | undefined;
    await act(async () => { renderer = create(<SidebarProvider><Probe /></SidebarProvider>); });
    await flush();

    expect(shown(null)).toEqual(['recorded', 'transcribed', 'done', 'recap']);
    expect(shown('transcribed')).toEqual(['recorded']);
    expect(shown('summarized')).toEqual(['recorded', 'transcribed']);
    expect(shown('obsidianExported')).toEqual(['recorded', 'transcribed']);

    backendMeetings = backendMeetings.map(m => m.id === 'transcribed' ? { ...m, summarized: true } : m);
    await act(async () => { handlers.get('meetings-changed')!({ payload: { meeting_id: 'transcribed' } }); });
    await flush();
    expect(shown('summarized')).toEqual(['recorded']);

    backendMeetings = backendMeetings.map(m => m.id === 'recorded' ? { ...m, transcribed: true, transcript_needed: false } : m);
    await act(async () => { handlers.get('meetings-changed')!({ payload: { meeting_id: 'recorded' } }); });
    await flush();
    expect(shown('transcribed')).toEqual([]);

    await act(async () => { renderer!.unmount(); });
    expect(handlers.has('meetings-changed')).toBe(false);
  });
});

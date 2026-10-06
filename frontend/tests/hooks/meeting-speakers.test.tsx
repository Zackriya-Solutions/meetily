import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSpeaker, NameOutcome, PropagatedLink } from '../../src/types';
import { makeSpeaker } from '../fixtures/speakers';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('sonner', () => originalToast);
});

let listed: MeetingSpeaker[] = [];
let outcome: NameOutcome = { person_id: null, propagated: [] };
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_list_meeting_speakers') return listed;
  if (command === 'api_name_meeting_speaker' || command === 'api_confirm_meeting_speaker_name') return outcome;
  if (command === 'api_reject_meeting_speaker_name') return null;
  if (command === 'api_undo_name_propagation') return ['meeting-b', 'meeting-c'];
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
type ToastOptions = { action?: { label: string; onClick: () => void }; duration?: number };
const success = mock((_message: string, _options?: ToastOptions) => {});
const failure = mock((_message: string) => {});
mock.module('sonner', () => ({ toast: { success, error: failure, info: () => {}, warning: () => {} } }));
const { useMeetingSpeakers } = await import('../../src/hooks/useMeetingSpeakers');

let state: ReturnType<typeof useMeetingSpeakers>;
let renderer: ReactTestRenderer | undefined;
function View() {
  state = useMeetingSpeakers('meeting-a');
  return null;
}
const links: PropagatedLink[] = [
  { meeting_id: 'meeting-b', speaker_key: 'spk_0', person_id: 'person-1' },
  { meeting_id: 'meeting-b', speaker_key: 'spk_3', person_id: 'person-1' },
  { meeting_id: 'meeting-c', speaker_key: 'spk_1', person_id: 'person-1' },
];

beforeEach(async () => {
  listed = [makeSpeaker('spk_1')];
  outcome = { person_id: null, propagated: [] };
  invoke.mockClear();
  success.mockClear();
  failure.mockClear();
  await act(async () => { renderer = create(<View />); });
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
});

describe('meeting speaker naming', () => {
  test('naming shows undo toast for other meetings', async () => {
    outcome = { person_id: 'person-1', propagated: links };
    await act(async () => { await state.name('spk_1', 'Noah'); });
    expect(invoke).toHaveBeenCalledWith('api_name_meeting_speaker', { meetingId: 'meeting-a', speakerKey: 'spk_1', name: 'Noah' });
    expect(success).toHaveBeenCalledTimes(1);
    const [message, options] = success.mock.calls[0];
    expect(message).toBe('Also named in 2 other meetings');
    expect(options?.action?.label).toBe('Undo');
    expect(options?.duration).toBe(10000);
  });

  test('undo sends exactly the propagated links', async () => {
    outcome = { person_id: 'person-1', propagated: links };
    await act(async () => { await state.confirm('spk_1'); });
    expect(invoke).toHaveBeenCalledWith('api_confirm_meeting_speaker_name', { meetingId: 'meeting-a', speakerKey: 'spk_1' });
    const options = success.mock.calls[0][1];
    await act(async () => {
      options!.action!.onClick();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(invoke).toHaveBeenCalledWith('api_undo_name_propagation', { links });
    expect(failure).not.toHaveBeenCalled();
  });

  test('no toast when nothing propagated', async () => {
    await act(async () => { await state.name('spk_1', 'Noah'); });
    await act(async () => { await state.reject('spk_1'); });
    expect(invoke).toHaveBeenCalledWith('api_reject_meeting_speaker_name', { meetingId: 'meeting-a', speakerKey: 'spk_1' });
    expect(success).not.toHaveBeenCalled();
  });

  test('refetch returns fresh speakers', async () => {
    listed = [makeSpeaker('spk_0', { display_name: 'Ana' }), makeSpeaker('spk_1')];
    let fresh: MeetingSpeaker[] = [];
    await act(async () => { fresh = await state.refetch(); });
    expect(fresh).toEqual(listed);
    expect(state.speakers).toEqual(listed);
    expect(state.names).toEqual({ spk_0: 'Ana', spk_1: 'Speaker 2' });
  });
});

import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { MeetingSpeaker } from '../../src/types';

const originalPopover = { ...await import('../../src/components/ui/popover') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('../../src/components/ui/popover', () => originalPopover);
  mock.module('sonner', () => originalToast);
});
// Render popover content inline so its buttons can be pressed without a DOM.
const passthrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: passthrough, PopoverTrigger: passthrough, PopoverContent: passthrough, PopoverAnchor: passthrough,
}));
mock.module('sonner', () => ({ toast: { error: () => {}, success: () => {}, info: () => {}, warning: () => {} } }));
const { SpeakerChip } = await import('../../src/components/Speakers/SpeakerChip');

const speakers: MeetingSpeaker[] = [
  { speaker_key: 'spk_0', display_name: null, speech_seconds: 6, row_count: 3, row_seconds: 6 },
  { speaker_key: 'spk_1', display_name: 'Ana', speech_seconds: 2, row_count: 1, row_seconds: 2 },
];
const noop = async () => {};

describe('compact speaker control', () => {
  test('reassigns the row it sits on, inside a same-speaker run', async () => {
    const onReassign = mock(async (_id: string, _key: string | null) => {});
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SpeakerChip compact speakerKey="spk_0" transcriptId="t2" speakers={speakers} names={{ spk_1: 'Ana' }}
          editable onRename={noop} onMerge={noop} onReassign={onReassign} />,
      );
    });
    expect(renderer.root.findAll((n) => n.type === 'button' && n.props['aria-label'] === 'Change speaker (Speaker 1)')).toHaveLength(1);
    const target = renderer.root.find((n) => n.type === 'button' && n.props['aria-label'] === 'This line was said by Ana');
    await act(async () => { target.props.onClick(); });
    expect(onReassign).toHaveBeenCalledWith('t2', 'spk_1');
  });

  test('renders nothing when editing is off', async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SpeakerChip compact speakerKey="spk_0" transcriptId="t2" speakers={speakers} names={{}}
          editable={false} onRename={noop} onMerge={noop} onReassign={noop} />,
      );
    });
    expect(renderer.toJSON()).toBeNull();
  });
});

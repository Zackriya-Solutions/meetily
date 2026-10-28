import { expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { createRef, forwardRef, useImperativeHandle, type ComponentProps } from 'react';
import type { BlockNoteSummaryViewRef } from '../../src/components/AISummary/BlockNoteSummaryView';

const save = mock(async () => {});
const copy = mock(async () => 'Saved summary text');
mock.module('../../src/components/AISummary/BlockNoteSummaryView', () => ({
  BlockNoteSummaryView: forwardRef((_props, ref) => {
    useImperativeHandle(ref, () => ({ saveSummary: save, getMarkdown: copy, isDirty: false }));
    return <div data-editor="loaded">Summary content</div>;
  }),
}));
mock.module('../../src/components/MeetingDetails/SummaryGeneratorButtonGroup', () => ({
  SummaryGeneratorButtonGroup: () => <button>Generate Summary</button>,
}));
mock.module('../../src/components/MeetingDetails/SummaryUpdaterButtonGroup', () => ({
  SummaryUpdaterButtonGroup: ({ onSave, onCopy }: { onSave: () => Promise<void>; onCopy: () => Promise<void> }) => <>
    <button onClick={onSave}>Save Changes</button><button onClick={onCopy}>Copy Summary</button>
  </>,
}));
mock.module('../../src/components/EmptyStateSummary', () => ({ EmptyStateSummary: () => <div>No summary yet</div> }));
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ children }: { children: React.ReactNode }) => children,
  PopoverTrigger: ({ children }: { children: React.ReactNode }) => children,
  PopoverContent: () => null,
}));
mock.module('../../src/hooks/useRecentLanguages', () => ({ useRecentLanguages: () => ({ addRecent() {} }) }));
mock.module('../../src/lib/summary-language-preferences', () => ({
  readMeetingSummaryLanguage: async () => ({ language: null, storage: 'metadata' }),
  saveMeetingSummaryLanguage: async () => {},
}));

const { SummaryPanel } = await import('../../src/components/MeetingDetails/SummaryPanel');

test('deferred summary editor keeps empty state, loading status, and Save/Copy ref', async () => {
  const summaryRef = createRef<BlockNoteSummaryViewRef>();
  const props = {
    meeting: { id: 'meeting', title: 'Test', created_at: '2026-10-09' },
    meetingTitle: 'Test', aiSummary: null, summaryStatus: 'idle', transcripts: [], summaryRef,
    modelConfig: { provider: 'ollama', model: 'test' },
    onSaveAll: async () => { await summaryRef.current!.saveSummary(); },
    onCopySummary: async () => { await summaryRef.current!.getMarkdown(); },
    getSummaryStatusMessage: () => 'Summary completed',
  } as unknown as ComponentProps<typeof SummaryPanel>;
  let renderer!: ReactTestRenderer;
  try {
    await act(async () => { renderer = create(<SummaryPanel {...props} />); });
    expect(JSON.stringify(renderer.toJSON())).toContain('No summary yet');
    expect(summaryRef.current).toBeNull();
    act(() => renderer.update(<SummaryPanel {...props} aiSummary={{ markdown: 'Summary text' } as never} summaryStatus="completed" />));
    expect(renderer.root.findByProps({ role: 'status' }).children).toContain('Loading summary editor...');
    await act(async () => {});
    expect(renderer.root.findByProps({ 'data-editor': 'loaded' })).toBeDefined();
    const buttons = renderer.root.findAllByType('button');
    await act(async () => {
      await buttons.find(button => button.children.includes('Save Changes'))!.props.onClick();
      await buttons.find(button => button.children.includes('Copy Summary'))!.props.onClick();
    });
    expect(save).toHaveBeenCalledTimes(1);
    expect(copy).toHaveBeenCalledTimes(1);
    act(() => renderer.unmount());
    expect(summaryRef.current).toBeNull();
  } finally {
    if (renderer) act(() => renderer.unmount());
  }
});

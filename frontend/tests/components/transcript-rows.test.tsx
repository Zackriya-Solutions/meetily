import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { TranscriptSegmentData } from '../../src/types';

const originalTooltip = { ...await import('../../src/components/ui/tooltip') };
afterAll(() => mock.module('../../src/components/ui/tooltip', () => originalTooltip));
// Radix tooltips need a provider; the rows only need their children.
const passthrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/tooltip', () => ({
  ...originalTooltip, Tooltip: passthrough, TooltipTrigger: passthrough, TooltipContent: passthrough,
}));
const { VirtualizedTranscriptView } = await import('../../src/components/VirtualizedTranscriptView');
type RenderSpeaker = NonNullable<Parameters<typeof VirtualizedTranscriptView>[0]['renderSpeaker']>;

const segments: TranscriptSegmentData[] = [
  { id: 't1', timestamp: 0, text: 'hello', speaker: 'spk_0' },
  { id: 't2', timestamp: 2, text: 'again', speaker: 'spk_0' },
  { id: 't3', timestamp: 4, text: 'reply', speaker: 'spk_1' },
  { id: 't4', timestamp: 6, text: 'unlabelled', speaker: null },
];

describe('transcript rows', () => {
  test('re-render only when what they show changes', async () => {
    const calls: Array<[string | null, string, boolean]> = [];
    const renderSpeaker: RenderSpeaker = (key, id, runStart) => {
      calls.push([key, id, runStart]);
      return key ? <span>{key}</span> : null;
    };
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<VirtualizedTranscriptView segments={segments} renderSpeaker={renderSpeaker} disableAutoScroll totalCount={4} />);
    });
    expect(calls).toEqual([
      ['spk_0', 't1', true],
      ['spk_0', 't2', false],
      ['spk_1', 't3', true],
      [null, 't4', false],
    ]);

    // A parent re-render with the same rows and the same renderer (a progress event, a scroll) leaves rows alone.
    calls.length = 0;
    await act(async () => {
      renderer.update(<VirtualizedTranscriptView segments={segments} renderSpeaker={renderSpeaker} disableAutoScroll totalCount={5} />);
    });
    expect(calls).toEqual([]);

    // A new renderer (speakers renamed, editing toggled) reaches every row.
    await act(async () => {
      renderer.update(<VirtualizedTranscriptView segments={segments} renderSpeaker={(...args) => renderSpeaker(...args)} disableAutoScroll totalCount={5} />);
    });
    expect(calls).toHaveLength(4);
  });
});

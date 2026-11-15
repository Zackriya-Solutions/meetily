import { describe, expect, mock, test } from 'bun:test';

mock.module('@tauri-apps/api/core', () => ({ invoke: async () => [] }));

const {
  defaultSpeakerLabel,
  speakerDisplayName,
  speakerColorClasses,
  speakersInOrder,
  speakerTextPrefix,
} = await import('../../src/lib/speakers');

describe('speaker labels', () => {
  test('maps stored ids to default labels', () => {
    expect(defaultSpeakerLabel('me')).toBe('Me');
    expect(defaultSpeakerLabel('others')).toBe('Others');
    expect(defaultSpeakerLabel('speaker_3')).toBe('Speaker 3');
    expect(defaultSpeakerLabel('custom')).toBe('custom');
  });

  test('prefers custom names and ignores blank ones', () => {
    expect(speakerDisplayName('speaker_1', { speaker_1: 'Alice' })).toBe('Alice');
    expect(speakerDisplayName('speaker_1', { speaker_1: '  ' })).toBe('Speaker 1');
    expect(speakerDisplayName('me')).toBe('Me');
  });

  test('builds plain-text prefixes for copy and summary', () => {
    expect(speakerTextPrefix('speaker_2', { speaker_2: 'Bob' })).toBe('Bob: ');
    expect(speakerTextPrefix(null)).toBe('');
    expect(speakerTextPrefix(undefined)).toBe('');
  });

  test('lists speakers by first appearance', () => {
    expect(
      speakersInOrder([
        { speaker: 'speaker_2' },
        { speaker: null },
        { speaker: 'me' },
        { speaker: 'speaker_2' },
        {},
      ])
    ).toEqual(['speaker_2', 'me']);
  });

  test('assigns stable, distinct colors', () => {
    expect(speakerColorClasses('speaker_1')).toBe(speakerColorClasses('speaker_1'));
    expect(speakerColorClasses('speaker_1')).not.toBe(speakerColorClasses('speaker_2'));
    expect(speakerColorClasses('me')).toContain('blue');
  });
});

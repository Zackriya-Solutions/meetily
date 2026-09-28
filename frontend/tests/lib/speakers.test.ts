import { describe, expect, test } from 'bun:test';
import { buildSpeakerNameMap, defaultSpeakerLabel, formatTranscriptLine, isSpeakerRunStart } from '../../src/lib/speakers';
import type { MeetingSpeaker, Transcript } from '../../src/types';

const speaker = (speaker_key: string, display_name: string | null): MeetingSpeaker => ({
  speaker_key, display_name, speech_seconds: 1, row_count: 1, row_seconds: 1,
});

describe('speaker helpers', () => {
  test('default labels are one-based', () => {
    expect(defaultSpeakerLabel('spk_0')).toBe('Speaker 1');
    expect(defaultSpeakerLabel('spk_4')).toBe('Speaker 5');
    expect(defaultSpeakerLabel('weird')).toBe('weird');
  });

  test('blank names fall back to the default label', () => {
    expect(buildSpeakerNameMap([speaker('spk_0', '  '), speaker('spk_1', 'Ana')])).toEqual({ spk_0: 'Speaker 1', spk_1: 'Ana' });
  });

  test('lines carry the speaker name only when the row has a speaker', () => {
    const row = { id: 't1', text: 'hello', timestamp: '10:00', audio_start_time: 65 } as Transcript;
    expect(formatTranscriptLine({ ...row, speaker: 'spk_1' }, { spk_1: 'Ana' })).toBe('[01:05] Ana: hello');
    expect(formatTranscriptLine({ ...row, speaker: null }, {})).toBe('[01:05] hello');
    expect(formatTranscriptLine({ ...row, audio_start_time: undefined }, {})).toBe('10:00 hello');
  });

  test('only the first row of a same-speaker run starts a run', () => {
    const rows = [{ speaker: 'spk_0' }, { speaker: 'spk_0' }, { speaker: 'spk_0' }, { speaker: null }, { speaker: 'spk_1' }];
    expect(rows.map((_, i) => isSpeakerRunStart(rows, i))).toEqual([true, false, false, false, true]);
  });
});

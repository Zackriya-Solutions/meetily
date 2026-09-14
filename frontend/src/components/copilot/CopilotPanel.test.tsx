// @vitest-environment jsdom

/**
 * What the copilot panel must never do, as tests rather than as comments
 * (BACKLOG I1, ADR-0038).
 *
 * Most of these assert an *absence*, which is the kind of guarantee that rots
 * quietly. Nothing fails on its own when a "Start recording" button is added to
 * a panel that is supposed to follow a session the user already consented to;
 * when a screen-sharing chip starts promising invisibility; when the chip keeps
 * printing the platform's verdict after the user switched the request off; or
 * when a speaker label reappears on a transcript line that the pipeline cannot
 * actually attribute. Every one of them was confirmed load-bearing by mutating
 * the source and watching the test fail before being committed.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { CopilotPanel, appendLine, TAIL_LENGTH, PanelLine } from './CopilotPanel';
import type { CopilotStatus } from '@/types/copilot';
import type { TranscriptUpdate } from '@/types';

// The suite default is `environment: 'node'`; this file opts into jsdom above.
// Cleanup is explicit because `globals` is off, so RTL's automatic afterEach
// hook is never installed — without this, each render stacks on the last and
// every `getByText` finds two of everything.
afterEach(cleanup);

const BASE_STATUS: CopilotStatus = {
  config: {
    enabled: true,
    contentProtection: true,
    shortcutsEnabled: true,
    keybinds: {
      togglePanel: 'CommandOrControl+Shift+M',
      ask: 'CommandOrControl+Shift+A',
      captureScreen: 'CommandOrControl+Shift+S',
    },
    liveWindowSecs: 180,
    allowCloudInsights: false,
  },
  protection: {
    level: 'bestEffort',
    headline: 'Best effort on macOS',
    detail: 'macOS 15 and newer ignore this for ScreenCaptureKit-based sharing.',
  },
  panelOpen: true,
  recording: true,
  shortcuts: [],
  liveContext: {
    subscribed: false,
    windowSecs: 0,
    turns: 0,
    windowTurns: 0,
    evicted: 0,
  },
  liveActions: ['suggest', 'followUpQuestions', 'recap', 'define'],
};

function update(overrides: Partial<TranscriptUpdate>): TranscriptUpdate {
  return {
    text: 'hello',
    timestamp: '14:00:00',
    source: 'system',
    sequence_id: 1,
    chunk_start_time: 0,
    is_partial: false,
    confidence: 0.9,
    audio_start_time: 0,
    audio_end_time: 1,
    duration: 1,
    ...overrides,
  };
}

describe('transcript tail', () => {
  it('stores a segment without inventing a speaker', () => {
    // `source` is the only field that could tempt a me/them split, and the sole
    // producer of `transcript-update` hardcodes it to "Audio" for microphone and
    // system audio alike — so a line carries text and provenance, never a guess
    // about who spoke (ADR-0034).
    const [line] = appendLine([], update({ text: 'hello', source: 'Audio' }));
    expect(Object.keys(line).sort()).toEqual(['id', 'text', 'timestamp']);
  });

  it('keeps the short chunks the producer flags as partial', () => {
    // I1 dropped these, believing `is_partial` meant "about to be replaced by a
    // final". It does not: Whisper sets it for any chunk under 15 s
    // (`whisper_engine.rs`), every chunk is emitted exactly once, and live VAD
    // closes a segment after 400 ms of silence — so most real speech is
    // "partial".
    // Dropping it hid most of the meeting from the panel.
    expect(appendLine([], update({ is_partial: true, text: 'a short utterance' }))).toHaveLength(1);
  });

  it('ignores empty text', () => {
    expect(appendLine([], update({ text: '   ' }))).toEqual([]);
  });

  it('replaces a segment when it arrives again rather than duplicating it', () => {
    const first = appendLine([], update({ sequence_id: 7, text: 'draft' }));
    const second = appendLine(first, update({ sequence_id: 7, text: 'final' }));
    expect(second).toHaveLength(1);
    expect(second[0].text).toBe('final');
  });

  it('keeps the tail bounded', () => {
    let lines: PanelLine[] = [];
    for (let i = 0; i < TAIL_LENGTH + 5; i += 1) {
      lines = appendLine(lines, update({ sequence_id: i, text: `line ${i}` }));
    }
    expect(lines).toHaveLength(TAIL_LENGTH);
    expect(lines[lines.length - 1].text).toBe(`line ${TAIL_LENGTH + 4}`);
  });
});

describe('the panel', () => {
  it('offers no way to start a recording', () => {
    // ADR-0038 invariant 2: the copilot follows a session whose consent was
    // already taken in Rust. It must never be a second way to begin capture.
    render(<CopilotPanel status={{ ...BASE_STATUS, recording: false }} lines={[]} />);
    const startish = screen
      .queryAllByRole('button')
      .filter((button) => /start|record/i.test(button.textContent ?? ''));
    expect(startish).toEqual([]);
  });

  it('says the copilot does not record on its own when nothing is running', () => {
    render(<CopilotPanel status={{ ...BASE_STATUS, recording: false }} lines={[]} />);
    expect(screen.getByText(/never records on its own/i)).toBeTruthy();
  });

  it('shows the platform verdict verbatim and never claims invisibility', () => {
    render(<CopilotPanel status={BASE_STATUS} lines={[]} />);
    expect(screen.getByText('Best effort on macOS')).toBeTruthy();
    expect(document.body.textContent ?? '').not.toMatch(/undetectable|invisible/i);
  });

  it('renders the tail with nothing attached to the words but the words', () => {
    // Exact text, not a substring: a re-added "You"/"Them" prefix would make the
    // list item read "Themtheir line" and fail here. A timestamp printed beside
    // the text would fail here too — it is UTC from the worker, so it must not
    // be shown as if it were a local clock time.
    const lines: PanelLine[] = [
      { id: 1, text: 'their line', timestamp: '14:00:00' },
      { id: 2, text: 'my line', timestamp: '14:00:05' },
    ];
    render(<CopilotPanel status={BASE_STATUS} lines={lines} />);
    expect(screen.getAllByRole('listitem').map((item) => item.textContent)).toEqual([
      'their line',
      'my line',
    ]);
  });

  it('reports the panel as visible when the user switched hiding off', () => {
    // The bug this pins: the chip read the platform verdict only, so a user who
    // turned the setting off still saw the platform's capability reported as
    // fact — on Windows, literally "Hidden from screen sharing" over a panel
    // that is not.
    const off = {
      ...BASE_STATUS,
      config: { ...BASE_STATUS.config, contentProtection: false },
    };
    render(<CopilotPanel status={off} lines={[]} />);
    expect(screen.getByText(/visible in screen shares/i)).toBeTruthy();
    expect(screen.queryByText(BASE_STATUS.protection.headline)).toBeNull();
  });

  it('never shows an enforced verdict while the request is switched off', () => {
    // The dangerous direction of the same bug: on the one platform that really
    // does exclude the window, the chip must still not claim it.
    const off = {
      ...BASE_STATUS,
      config: { ...BASE_STATUS.config, contentProtection: false },
      protection: {
        level: 'enforced' as const,
        headline: 'Hidden from screen sharing',
        detail: 'Windows excludes this panel from screen sharing and recording.',
      },
    };
    render(<CopilotPanel status={off} lines={[]} />);
    expect(screen.queryByText('Hidden from screen sharing')).toBeNull();
    expect(document.body.textContent ?? '').not.toMatch(/hidden from screen/i);
  });
});

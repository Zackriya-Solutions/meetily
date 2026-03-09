import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { TranscriptProvider, useTranscripts } from './TranscriptContext';
import { RecordingStateProvider } from './RecordingStateContext';

// Mock services
const mockOnTranscriptUpdate = vi.fn();
const mockGetTranscriptHistory = vi.fn();

vi.mock('@/services/transcriptService', () => ({
  transcriptService: {
    onTranscriptUpdate: (...args: any[]) => mockOnTranscriptUpdate(...args),
    getTranscriptHistory: (...args: any[]) => mockGetTranscriptHistory(...args),
  },
}));

const mockOnRecordingStarted = vi.fn();
const mockOnRecordingStopped = vi.fn();
const mockOnRecordingPaused = vi.fn();
const mockOnRecordingResumed = vi.fn();
const mockGetRecordingState = vi.fn();
const mockGetRecordingMeetingName = vi.fn();

vi.mock('@/services/recordingService', () => ({
  recordingService: {
    onRecordingStarted: (...args: any[]) => mockOnRecordingStarted(...args),
    onRecordingStopped: (...args: any[]) => mockOnRecordingStopped(...args),
    onRecordingPaused: (...args: any[]) => mockOnRecordingPaused(...args),
    onRecordingResumed: (...args: any[]) => mockOnRecordingResumed(...args),
    getRecordingState: (...args: any[]) => mockGetRecordingState(...args),
    getRecordingMeetingName: (...args: any[]) => mockGetRecordingMeetingName(...args),
  },
}));

vi.mock('@/services/indexedDBService', () => ({
  indexedDBService: {
    init: vi.fn().mockResolvedValue(undefined),
    saveMeetingMetadata: vi.fn().mockResolvedValue(undefined),
    getMeetingMetadata: vi.fn().mockResolvedValue(null),
    saveTranscript: vi.fn().mockResolvedValue(undefined),
    markMeetingSaved: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('@/lib/clipboard', () => ({
  copyToClipboard: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

// Helper consumer component
function TestConsumer() {
  const ctx = useTranscripts();
  return (
    <div>
      <span data-testid="count">{ctx.transcripts.length}</span>
      <span data-testid="title">{ctx.meetingTitle}</span>
      <span data-testid="meetingId">{ctx.currentMeetingId || 'null'}</span>
      <span data-testid="transcripts">{JSON.stringify(ctx.transcripts.map(t => t.text))}</span>
      <button
        onClick={() =>
          ctx.addTranscript({
            text: 'Hello world',
            timestamp: '14:30:00',
            source: 'mic',
            sequence_id: 1,
            chunk_start_time: 0,
            is_partial: false,
            confidence: 0.95,
            audio_start_time: 0,
            audio_end_time: 3,
            duration: 3,
          })
        }
      >
        AddTranscript
      </button>
      <button
        onClick={() =>
          ctx.addTranscript({
            text: 'Second entry',
            timestamp: '14:30:05',
            source: 'mic',
            sequence_id: 2,
            chunk_start_time: 3,
            is_partial: false,
            confidence: 0.9,
            audio_start_time: 3,
            audio_end_time: 6,
            duration: 3,
          })
        }
      >
        AddSecond
      </button>
      <button onClick={() => ctx.clearTranscripts()}>Clear</button>
      <button onClick={() => ctx.setMeetingTitle('New Title')}>SetTitle</button>
      <button onClick={() => ctx.copyTranscript()}>Copy</button>
      <button onClick={() => ctx.flushBuffer()}>Flush</button>
    </div>
  );
}

function renderWithProvider() {
  return render(
    <RecordingStateProvider>
      <TranscriptProvider>
        <TestConsumer />
      </TranscriptProvider>
    </RecordingStateProvider>
  );
}

describe('TranscriptContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockOnTranscriptUpdate.mockResolvedValue(() => {});
    mockGetTranscriptHistory.mockResolvedValue([]);
    mockOnRecordingStarted.mockResolvedValue(() => {});
    mockOnRecordingStopped.mockResolvedValue(() => {});
    mockOnRecordingPaused.mockResolvedValue(() => {});
    mockOnRecordingResumed.mockResolvedValue(() => {});
    mockGetRecordingState.mockResolvedValue({
      is_recording: false,
      is_paused: false,
      is_active: false,
      recording_duration: null,
      active_duration: null,
    });
    mockGetRecordingMeetingName.mockResolvedValue(null);
  });

  describe('useTranscripts hook', () => {
    it('throws when used outside of TranscriptProvider', () => {
      const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
      // Need RecordingStateProvider but NOT TranscriptProvider
      expect(() =>
        render(
          <RecordingStateProvider>
            <TestConsumer />
          </RecordingStateProvider>
        )
      ).toThrow('useTranscripts must be used within a TranscriptProvider');
      spy.mockRestore();
    });
  });

  describe('initial state', () => {
    it('starts with empty transcripts', () => {
      renderWithProvider();
      expect(screen.getByTestId('count').textContent).toBe('0');
    });

    it('starts with default meeting title', () => {
      renderWithProvider();
      expect(screen.getByTestId('title').textContent).toBe('+ New Call');
    });

    it('starts with null meeting ID', () => {
      renderWithProvider();
      expect(screen.getByTestId('meetingId').textContent).toBe('null');
    });
  });

  describe('addTranscript', () => {
    it('adds a transcript to the list', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      expect(screen.getByTestId('count').textContent).toBe('0');

      await user.click(screen.getByText('AddTranscript'));
      expect(screen.getByTestId('count').textContent).toBe('1');
      expect(screen.getByTestId('transcripts').textContent).toContain('Hello world');
    });

    it('adds multiple transcripts in order', async () => {
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('AddTranscript'));
      await user.click(screen.getByText('AddSecond'));

      expect(screen.getByTestId('count').textContent).toBe('2');
      const transcripts = JSON.parse(screen.getByTestId('transcripts').textContent!);
      expect(transcripts).toEqual(['Hello world', 'Second entry']);
    });

    it('deduplicates transcripts with same text and timestamp', async () => {
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('AddTranscript'));
      await user.click(screen.getByText('AddTranscript'));

      expect(screen.getByTestId('count').textContent).toBe('1');
    });
  });

  describe('clearTranscripts', () => {
    it('clears all transcripts', async () => {
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('AddTranscript'));
      expect(screen.getByTestId('count').textContent).toBe('1');

      await user.click(screen.getByText('Clear'));
      expect(screen.getByTestId('count').textContent).toBe('0');
    });
  });

  describe('setMeetingTitle', () => {
    it('updates the meeting title', async () => {
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('SetTitle'));
      expect(screen.getByTestId('title').textContent).toBe('New Title');
    });
  });

  describe('copyTranscript', () => {
    it('copies transcript text to clipboard', async () => {
      const { copyToClipboard } = await import('@/lib/clipboard');
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('AddTranscript'));
      await user.click(screen.getByText('Copy'));

      await waitFor(() => {
        expect(copyToClipboard).toHaveBeenCalled();
      });
    });
  });

  describe('listener setup', () => {
    it('sets up transcript listener on mount', async () => {
      renderWithProvider();
      await waitFor(() => {
        expect(mockOnTranscriptUpdate).toHaveBeenCalled();
      });
    });

    it('sets up recording listeners on mount', async () => {
      renderWithProvider();
      await waitFor(() => {
        expect(mockOnRecordingStarted).toHaveBeenCalled();
        expect(mockOnRecordingStopped).toHaveBeenCalled();
      });
    });
  });
});

import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// Mock the types import
vi.mock('@/types', () => ({}));

import { TranscriptService, transcriptService } from './transcriptService';

describe('TranscriptService', () => {
  let service: TranscriptService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new TranscriptService();
  });

  describe('getTranscriptHistory', () => {
    it('should invoke get_transcript_history and return transcripts', async () => {
      const transcripts = [
        { text: 'Hello world', timestamp: '2025-01-01T00:00:00Z', confidence: 0.9 },
        { text: 'Goodbye', timestamp: '2025-01-01T00:01:00Z', confidence: 0.85 },
      ];
      vi.mocked(invoke).mockResolvedValueOnce(transcripts);

      const result = await service.getTranscriptHistory();
      expect(result).toEqual(transcripts);
      expect(invoke).toHaveBeenCalledWith('get_transcript_history');
    });

    it('should return empty array when no history', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([]);

      const result = await service.getTranscriptHistory();
      expect(result).toEqual([]);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('History unavailable'));
      await expect(service.getTranscriptHistory()).rejects.toThrow('History unavailable');
    });
  });

  describe('getTranscriptionStatus', () => {
    it('should invoke get_transcription_status and return status', async () => {
      const status = { chunks_in_queue: 5, is_processing: true, last_activity_ms: 1500 };
      vi.mocked(invoke).mockResolvedValueOnce(status);

      const result = await service.getTranscriptionStatus();
      expect(result).toEqual(status);
      expect(invoke).toHaveBeenCalledWith('get_transcription_status');
    });

    it('should return idle status', async () => {
      const status = { chunks_in_queue: 0, is_processing: false, last_activity_ms: 30000 };
      vi.mocked(invoke).mockResolvedValueOnce(status);

      const result = await service.getTranscriptionStatus();
      expect(result.is_processing).toBe(false);
      expect(result.chunks_in_queue).toBe(0);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('status fail'));
      await expect(service.getTranscriptionStatus()).rejects.toThrow('status fail');
    });
  });

  describe('onTranscriptUpdate', () => {
    it('should listen for transcript-update events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onTranscriptUpdate(callback);

      expect(listen).toHaveBeenCalledWith('transcript-update', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should pass event payload to callback', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onTranscriptUpdate(callback);

      const update = { text: 'New segment', timestamp: 'now', confidence: 0.95 };
      capturedHandler({ payload: update });
      expect(callback).toHaveBeenCalledWith(update);
    });
  });

  describe('onTranscriptionComplete', () => {
    it('should listen for transcription-complete events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onTranscriptionComplete(callback);

      expect(listen).toHaveBeenCalledWith('transcription-complete', callback);
      expect(result).toBe(unlisten);
    });
  });

  describe('onTranscriptionError', () => {
    it('should listen for transcription-error events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onTranscriptionError(callback);

      expect(listen).toHaveBeenCalledWith('transcription-error', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should pass error payload to callback', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onTranscriptionError(callback);

      const errorPayload = { error: 'Model load fail', userMessage: 'Could not load model', actionable: true };
      capturedHandler({ payload: errorPayload });
      expect(callback).toHaveBeenCalledWith(errorPayload);
    });
  });

  describe('onTranscriptError', () => {
    it('should listen for transcript-error events (legacy)', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onTranscriptError(callback);

      expect(listen).toHaveBeenCalledWith('transcript-error', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should pass string error to callback', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onTranscriptError(callback);

      capturedHandler({ payload: 'Something went wrong' });
      expect(callback).toHaveBeenCalledWith('Something went wrong');
    });
  });

  describe('onModelDownloadComplete', () => {
    it('should listen for model-download-complete events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onModelDownloadComplete(callback);

      expect(listen).toHaveBeenCalledWith('model-download-complete', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should extract modelName from payload', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onModelDownloadComplete(callback);

      capturedHandler({ payload: { modelName: 'large-v3' } });
      expect(callback).toHaveBeenCalledWith('large-v3');
    });
  });

  describe('onParakeetModelDownloadComplete', () => {
    it('should listen for parakeet-model-download-complete events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onParakeetModelDownloadComplete(callback);

      expect(listen).toHaveBeenCalledWith('parakeet-model-download-complete', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should extract modelName from payload', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onParakeetModelDownloadComplete(callback);

      capturedHandler({ payload: { modelName: 'parakeet-v2' } });
      expect(callback).toHaveBeenCalledWith('parakeet-v2');
    });
  });

  describe('singleton export', () => {
    it('should export a singleton instance', () => {
      expect(transcriptService).toBeInstanceOf(TranscriptService);
    });
  });
});

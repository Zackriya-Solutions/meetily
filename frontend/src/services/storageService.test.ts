import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

// Mock the types import
vi.mock('@/types', () => ({}));

import { StorageService, storageService } from './storageService';

describe('StorageService', () => {
  let service: StorageService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new StorageService();
  });

  describe('saveMeeting', () => {
    it('should invoke api_save_transcript with correct params', async () => {
      const response = { meeting_id: 'meeting-123' };
      vi.mocked(invoke).mockResolvedValueOnce(response);

      const transcripts = [
        { text: 'Hello', timestamp: '2025-01-01T00:00:00Z', confidence: 0.95 },
      ] as any;

      const result = await service.saveMeeting('Team Standup', transcripts, '/path/to/folder');
      expect(result).toEqual(response);
      expect(invoke).toHaveBeenCalledWith('api_save_transcript', {
        meetingTitle: 'Team Standup',
        transcripts,
        folderPath: '/path/to/folder',
      });
    });

    it('should handle null folderPath', async () => {
      vi.mocked(invoke).mockResolvedValueOnce({ meeting_id: 'meeting-456' });

      await service.saveMeeting('Quick Chat', [], null);
      expect(invoke).toHaveBeenCalledWith('api_save_transcript', {
        meetingTitle: 'Quick Chat',
        transcripts: [],
        folderPath: null,
      });
    });

    it('should handle empty transcripts array', async () => {
      vi.mocked(invoke).mockResolvedValueOnce({ meeting_id: 'meeting-789' });

      const result = await service.saveMeeting('Empty Meeting', [], null);
      expect(result.meeting_id).toBe('meeting-789');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('Save failed'));
      await expect(service.saveMeeting('Test', [], null)).rejects.toThrow('Save failed');
    });
  });

  describe('getMeeting', () => {
    it('should invoke api_get_meeting with meeting ID', async () => {
      const meeting = { id: 'meeting-123', title: 'Standup', date: '2025-01-01' };
      vi.mocked(invoke).mockResolvedValueOnce(meeting);

      const result = await service.getMeeting('meeting-123');
      expect(result).toEqual(meeting);
      expect(invoke).toHaveBeenCalledWith('api_get_meeting', { meetingId: 'meeting-123' });
    });

    it('should return meeting with additional properties', async () => {
      const meeting = { id: 'm-1', title: 'Test', summary: 'A summary', duration: 3600 };
      vi.mocked(invoke).mockResolvedValueOnce(meeting);

      const result = await service.getMeeting('m-1');
      expect(result.summary).toBe('A summary');
      expect(result.duration).toBe(3600);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('Not found'));
      await expect(service.getMeeting('nonexistent')).rejects.toThrow('Not found');
    });
  });

  describe('getMeetings', () => {
    it('should invoke api_get_meetings and return array', async () => {
      const meetings = [
        { id: 'm-1', title: 'Meeting 1' },
        { id: 'm-2', title: 'Meeting 2' },
      ];
      vi.mocked(invoke).mockResolvedValueOnce(meetings);

      const result = await service.getMeetings();
      expect(result).toHaveLength(2);
      expect(result[0].title).toBe('Meeting 1');
      expect(invoke).toHaveBeenCalledWith('api_get_meetings');
    });

    it('should return empty array when no meetings', async () => {
      vi.mocked(invoke).mockResolvedValueOnce([]);

      const result = await service.getMeetings();
      expect(result).toEqual([]);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('DB error'));
      await expect(service.getMeetings()).rejects.toThrow('DB error');
    });
  });

  describe('singleton export', () => {
    it('should export a singleton instance', () => {
      expect(storageService).toBeInstanceOf(StorageService);
    });
  });
});

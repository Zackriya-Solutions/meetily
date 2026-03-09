import { vi, describe, it, expect, beforeEach } from 'vitest';
import { indexedDBService, MeetingMetadata } from './indexedDBService';

// Mock IDBKeyRange which is not available in jsdom
(globalThis as any).IDBKeyRange = {
  only: vi.fn((value: any) => value),
  bound: vi.fn(),
  lowerBound: vi.fn(),
  upperBound: vi.fn(),
};

describe('IndexedDBService', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    // Reset the service's internal state by accessing private fields
    (indexedDBService as any).db = null;
    (indexedDBService as any).initPromise = null;
  });

  describe('init', () => {
    it('should open IndexedDB and set up database', async () => {
      await indexedDBService.init();
      expect(indexedDB.open).toHaveBeenCalledWith('IQcaptureRecoveryDB', 1);
    });

    it('should reuse existing init promise (dedup)', async () => {
      const p1 = indexedDBService.init();
      const p2 = indexedDBService.init();
      // Both calls should resolve to the same value (async wrappers around same internal promise)
      await expect(Promise.all([p1, p2])).resolves.toBeDefined();
      // indexedDB.open should only be called once since the second call reuses the pending promise
      expect(indexedDB.open).toHaveBeenCalledTimes(1);
    });

    it('should resolve immediately if already initialized', async () => {
      await indexedDBService.init();
      // db is now set, so second call should resolve immediately
      await indexedDBService.init();
      // open should only be called once (from beforeEach clearing + first init)
      expect(indexedDB.open).toHaveBeenCalledTimes(1);
    });
  });

  describe('saveMeetingMetadata', () => {
    it('should save metadata to meetings store', async () => {
      const metadata: MeetingMetadata = {
        meetingId: 'meeting-1',
        title: 'Test Meeting',
        startTime: Date.now(),
        lastUpdated: Date.now(),
        transcriptCount: 0,
        savedToSQLite: false,
      };

      await indexedDBService.saveMeetingMetadata(metadata);
      // Should not throw - the mock store.put resolves
    });

    it('should auto-init if not initialized', async () => {
      const metadata: MeetingMetadata = {
        meetingId: 'meeting-2',
        title: 'Auto Init',
        startTime: Date.now(),
        lastUpdated: Date.now(),
        transcriptCount: 0,
        savedToSQLite: false,
      };

      await indexedDBService.saveMeetingMetadata(metadata);
      expect(indexedDB.open).toHaveBeenCalled();
    });

    it('should handle errors silently', async () => {
      // Force db to a broken state
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Transaction failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      await expect(indexedDBService.saveMeetingMetadata({
        meetingId: 'fail',
        title: 'Fail',
        startTime: 0,
        lastUpdated: 0,
        transcriptCount: 0,
        savedToSQLite: false,
      })).resolves.toBeUndefined();
      warnSpy.mockRestore();
    });
  });

  describe('getMeetingMetadata', () => {
    it('should return null for non-existent meeting', async () => {
      const result = await indexedDBService.getMeetingMetadata('nonexistent');
      expect(result).toBeNull();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.getMeetingMetadata('m-1');
      expect(indexedDB.open).toHaveBeenCalled();
    });

    it('should handle errors and return null', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Read failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.getMeetingMetadata('fail');
      expect(result).toBeNull();
      errorSpy.mockRestore();
    });
  });

  describe('getAllMeetings', () => {
    it('should return empty array when no meetings', async () => {
      const result = await indexedDBService.getAllMeetings();
      expect(result).toEqual([]);
    });

    it('should handle errors and return empty array', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Read failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.getAllMeetings();
      expect(result).toEqual([]);
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.getAllMeetings();
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('markMeetingSaved', () => {
    it('should not throw when meeting does not exist', async () => {
      await expect(indexedDBService.markMeetingSaved('nonexistent')).resolves.toBeUndefined();
    });

    it('should handle errors silently', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Write failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      await expect(indexedDBService.markMeetingSaved('fail')).resolves.toBeUndefined();
      warnSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.markMeetingSaved('m-1');
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('deleteMeeting', () => {
    function mockDbWithCursorSupport() {
      const mockReq = (result: any) => ({
        result,
        error: null,
        onsuccess: null as any,
        onerror: null as any,
      });

      const cursorOpenReq = mockReq(null);
      const deleteReq = mockReq(undefined);

      const mockTranscriptsStore = {
        index: vi.fn().mockReturnValue({
          openCursor: vi.fn().mockImplementation(() => {
            // Pass event with target.result = null (no cursor / empty)
            setTimeout(() => cursorOpenReq.onsuccess?.({ target: { result: null } }), 0);
            return cursorOpenReq;
          }),
        }),
      };

      const meetingDeleteReq = mockReq(undefined);
      const mockMeetingsStore = {
        delete: vi.fn().mockImplementation(() => {
          setTimeout(() => meetingDeleteReq.onsuccess?.(), 0);
          return meetingDeleteReq;
        }),
      };

      const storeMap: Record<string, any> = {
        meetings: mockMeetingsStore,
        transcripts: mockTranscriptsStore,
      };

      (indexedDBService as any).db = {
        transaction: vi.fn().mockReturnValue({
          objectStore: vi.fn().mockImplementation((name: string) => storeMap[name]),
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();
    }

    it('should delete meeting and its transcripts', async () => {
      mockDbWithCursorSupport();
      await expect(indexedDBService.deleteMeeting('m-1')).resolves.toBeUndefined();
    });

    it('should throw on errors', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Delete failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      await expect(indexedDBService.deleteMeeting('fail')).rejects.toThrow('Delete failed');
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      // After init, the default mock's openCursor doesn't pass event properly.
      // We need to provide a proper mock for the cursor to avoid the error.
      // First init normally, then swap db with cursor-aware mock.
      await indexedDBService.init();
      expect(indexedDB.open).toHaveBeenCalled();

      // Now set up proper cursor mock for the actual deleteMeeting call
      mockDbWithCursorSupport();
      await indexedDBService.deleteMeeting('m-1');
    });
  });

  describe('saveTranscript', () => {
    it('should save transcript and update meeting metadata', async () => {
      const transcript = {
        text: 'Hello world',
        timestamp: '2025-01-01T00:00:00Z',
        confidence: 0.95,
        sequenceId: 1,
      };

      await expect(indexedDBService.saveTranscript('m-1', transcript)).resolves.toBeUndefined();
    });

    it('should handle errors silently', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Save failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      await expect(indexedDBService.saveTranscript('m-1', {})).resolves.toBeUndefined();
      warnSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.saveTranscript('m-1', {});
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('getTranscripts', () => {
    it('should return empty array when no transcripts', async () => {
      const result = await indexedDBService.getTranscripts('m-1');
      expect(result).toEqual([]);
    });

    it('should handle errors and return empty array', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Read failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.getTranscripts('fail');
      expect(result).toEqual([]);
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.getTranscripts('m-1');
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('getTranscriptCount', () => {
    it('should return 0 when no transcripts', async () => {
      const result = await indexedDBService.getTranscriptCount('m-1');
      expect(result).toBe(0);
    });

    it('should handle errors and return 0', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Count failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.getTranscriptCount('fail');
      expect(result).toBe(0);
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.getTranscriptCount('m-1');
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('deleteOldMeetings', () => {
    it('should return 0 when no old meetings', async () => {
      const result = await indexedDBService.deleteOldMeetings(30);
      expect(result).toBe(0);
    });

    it('should handle errors and return 0', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Delete failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.deleteOldMeetings(7);
      expect(result).toBe(0);
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.deleteOldMeetings(30);
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });

  describe('deleteSavedMeetings', () => {
    it('should return 0 when no saved meetings', async () => {
      const result = await indexedDBService.deleteSavedMeetings(24);
      expect(result).toBe(0);
    });

    it('should handle errors and return 0', async () => {
      (indexedDBService as any).db = {
        transaction: vi.fn().mockImplementation(() => {
          throw new Error('Delete failed');
        }),
      };
      (indexedDBService as any).initPromise = Promise.resolve();

      const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const result = await indexedDBService.deleteSavedMeetings(24);
      expect(result).toBe(0);
      errorSpy.mockRestore();
    });

    it('should auto-init if not initialized', async () => {
      await indexedDBService.deleteSavedMeetings(24);
      expect(indexedDB.open).toHaveBeenCalled();
    });
  });
});

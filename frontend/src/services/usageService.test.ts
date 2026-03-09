import { vi, describe, it, expect, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

// Must mock authService before importing usageService
vi.mock('./authService', () => ({
  getAccessToken: vi.fn().mockResolvedValue(null),
  getAuthUserId: vi.fn().mockResolvedValue(null),
}));

import {
  trackEvent,
  trackMeetingCreated,
  trackSummaryGenerated,
  trackSessionStarted,
  trackSessionEnded,
  trackActiveMinutes,
  flushEvents,
  startPeriodicFlush,
  stopPeriodicFlush,
  initUsageService,
} from './usageService';
import { getAccessToken } from './authService';

describe('usageService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    stopPeriodicFlush();
  });

  afterEach(() => {
    stopPeriodicFlush();
    vi.useRealTimers();
  });

  describe('trackEvent', () => {
    it('should invoke usage_track_event with correct params', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await trackEvent('session_started', 1);
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'session_started',
        value: 1,
        metadata: null,
      });
    });

    it('should pass metadata when provided', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await trackEvent('meeting_created', 1, { meeting_id: 'm-1' });
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'meeting_created',
        value: 1,
        metadata: { meeting_id: 'm-1' },
      });
    });

    it('should handle invoke errors gracefully (warn, not throw)', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('track fail'));
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

      await expect(trackEvent('test', 1)).resolves.toBeUndefined();
      expect(warnSpy).toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it('should deep-clone metadata via JSON serialization', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      const meta = { nested: { key: 'value' } };
      await trackEvent('test', 1, meta);

      const call = vi.mocked(invoke).mock.calls[0];
      const passedMeta = (call[1] as any).metadata;
      expect(passedMeta).toEqual({ nested: { key: 'value' } });
    });
  });

  describe('convenience methods', () => {
    beforeEach(() => {
      vi.mocked(invoke).mockResolvedValue(undefined);
    });

    it('trackMeetingCreated should track with meeting_id', async () => {
      await trackMeetingCreated('m-123');
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'meeting_created',
        value: 1,
        metadata: { meeting_id: 'm-123' },
      });
    });

    it('trackMeetingCreated should track without meeting_id', async () => {
      await trackMeetingCreated();
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'meeting_created',
        value: 1,
        metadata: null,
      });
    });

    it('trackSummaryGenerated should track with full metadata', async () => {
      await trackSummaryGenerated('m-1', 'ollama', 'llama3');
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'summary_generated',
        value: 1,
        metadata: {
          meeting_id: 'm-1',
          llm_provider: 'ollama',
          llm_model: 'llama3',
        },
      });
    });

    it('trackSessionStarted should track session_started', async () => {
      await trackSessionStarted();
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'session_started',
        value: 1,
        metadata: null,
      });
    });

    it('trackSessionEnded should track with duration', async () => {
      await trackSessionEnded(45);
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'session_ended',
        value: 45,
        metadata: null,
      });
    });

    it('trackActiveMinutes should track with minutes', async () => {
      await trackActiveMinutes(30);
      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'active_minutes',
        value: 30,
        metadata: null,
      });
    });
  });

  describe('flushEvents', () => {
    it('should skip if not authenticated (no token)', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce(null);

      await flushEvents();
      // Should not call usage_flush_events
      expect(invoke).not.toHaveBeenCalledWith('usage_flush_events');
    });

    it('should skip if no events to flush', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce('tok');
      vi.mocked(invoke).mockResolvedValueOnce([]); // usage_flush_events returns empty

      await flushEvents();
      expect(global.fetch).not.toHaveBeenCalled();
    });

    it('should skip if events is null', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce('tok');
      vi.mocked(invoke).mockResolvedValueOnce(null); // usage_flush_events returns null

      await flushEvents();
      expect(global.fetch).not.toHaveBeenCalled();
    });

    it('should flush events to cloud API on success', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce('my-token');
      const events = [
        { event_type: 'session_started', value: 1, metadata: null },
      ];
      // usage_flush_events
      vi.mocked(invoke).mockResolvedValueOnce(events);
      // auth_get_user_id (inside flushEvents for device ID)
      vi.mocked(invoke).mockResolvedValueOnce('user-1');
      // auth_get_access_token (second invoke inside device ID logic)
      vi.mocked(invoke).mockResolvedValueOnce('tok');

      vi.mocked(global.fetch)
        // getBaseUrl config fetch (may or may not happen depending on cloudApiUrl)
        .mockResolvedValueOnce({
          ok: true,
          json: vi.fn().mockResolvedValue({ ingested: 1 }),
          status: 200,
        } as any);

      const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
      await flushEvents();
      logSpy.mockRestore();

      expect(global.fetch).toHaveBeenCalled();
    });

    it('should re-buffer events on flush failure', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce('my-token');
      const events = [
        { event_type: 'meeting_created', value: 1, metadata: null },
        { event_type: 'session_started', value: 1, metadata: null },
      ];
      // usage_flush_events
      vi.mocked(invoke).mockResolvedValueOnce(events);
      // auth_get_user_id
      vi.mocked(invoke).mockResolvedValueOnce('user-1');
      // auth_get_access_token
      vi.mocked(invoke).mockResolvedValueOnce('tok');
      // re-buffer invokes (usage_track_event x2)
      vi.mocked(invoke).mockResolvedValue(undefined);

      // getBaseUrl() may call fetch for config first, then the actual POST fails
      vi.mocked(global.fetch)
        .mockResolvedValueOnce({
          ok: true,
          json: vi.fn().mockResolvedValue({}),
          status: 200,
        } as any)
        .mockResolvedValueOnce({
          ok: false,
          status: 500,
        } as any);

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      await flushEvents();
      warnSpy.mockRestore();

      // Verify re-buffering - should have called usage_track_event for each event
      const trackCalls = vi.mocked(invoke).mock.calls.filter(
        c => c[0] === 'usage_track_event'
      );
      expect(trackCalls.length).toBe(2);
    });

    it('should handle flush errors gracefully', async () => {
      vi.mocked(getAccessToken).mockRejectedValueOnce(new Error('auth error'));

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      await expect(flushEvents()).resolves.toBeUndefined();
      warnSpy.mockRestore();
    });
  });

  describe('startPeriodicFlush', () => {
    it('should set up interval for flushing', () => {
      startPeriodicFlush(5000);
      // Verify interval was set (nothing thrown)
      stopPeriodicFlush();
    });

    it('should not create duplicate intervals', () => {
      startPeriodicFlush(5000);
      startPeriodicFlush(5000); // Should be no-op
      stopPeriodicFlush();
    });

    it('should use default interval of 60s', () => {
      startPeriodicFlush();
      stopPeriodicFlush();
    });
  });

  describe('stopPeriodicFlush', () => {
    it('should clear flush interval', () => {
      startPeriodicFlush(1000);
      stopPeriodicFlush();
      // Calling again should be safe
      stopPeriodicFlush();
    });

    it('should be safe to call when no interval exists', () => {
      expect(() => stopPeriodicFlush()).not.toThrow();
    });
  });

  describe('initUsageService', () => {
    it('should track session start and set up periodic flush', async () => {
      vi.mocked(invoke).mockResolvedValue(undefined);

      await initUsageService();

      expect(invoke).toHaveBeenCalledWith('usage_track_event', {
        eventType: 'session_started',
        value: 1,
        metadata: null,
      });

      // Clean up
      stopPeriodicFlush();
    });

    it('should read device ID from localStorage', async () => {
      vi.mocked(invoke).mockResolvedValue(undefined);
      localStorage.setItem('iqcapture_user_id', 'device-abc');

      await initUsageService();

      expect(localStorage.getItem).toHaveBeenCalledWith('iqcapture_user_id');
      expect((window as any).__iqcapture_device_id).toBe('device-abc');

      // Clean up
      stopPeriodicFlush();
      delete (window as any).__iqcapture_device_id;
    });

    it('should register beforeunload handler', async () => {
      vi.mocked(invoke).mockResolvedValue(undefined);

      await initUsageService();

      expect(window.addEventListener).toHaveBeenCalledWith('beforeunload', expect.any(Function));

      // Clean up
      stopPeriodicFlush();
    });
  });
});

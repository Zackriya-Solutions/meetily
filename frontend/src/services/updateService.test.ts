import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { UpdateService, updateService } from './updateService';

describe('UpdateService', () => {
  let service: UpdateService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new UpdateService();
  });

  describe('checkForUpdates', () => {
    it('should check for updates and return update info when available', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: true,
        current_version: '0.2.3',
        version: '0.3.0',
        date: '2025-06-01',
        body: 'New release',
        download_url: 'https://example.com/download',
        whats_new: ['Feature A', 'Bug fix B'],
      });

      const result = await service.checkForUpdates();
      expect(result.available).toBe(true);
      expect(result.currentVersion).toBe('0.2.3');
      expect(result.version).toBe('0.3.0');
      expect(result.downloadUrl).toBe('https://example.com/download');
      expect(result.whatsNew).toEqual(['Feature A', 'Bug fix B']);
      expect(invoke).toHaveBeenCalledWith('check_for_updates', { currentVersion: '0.2.3' });
    });

    it('should return no update available', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.3.0');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: false,
        current_version: '0.3.0',
      });

      const result = await service.checkForUpdates();
      expect(result.available).toBe(false);
      expect(result.version).toBeUndefined();
    });

    it('should prevent concurrent update checks', async () => {
      vi.mocked(getVersion).mockResolvedValue('0.2.3');
      vi.mocked(invoke).mockImplementation(() =>
        new Promise(resolve => setTimeout(() => resolve({
          available: false,
          current_version: '0.2.3',
        }), 100))
      );

      const firstCheck = service.checkForUpdates(true);
      await expect(service.checkForUpdates(true)).rejects.toThrow('Update check already in progress');

      await firstCheck;
    });

    it('should skip check if checked recently (not forced)', async () => {
      // First call sets lastCheckTime
      vi.mocked(getVersion).mockResolvedValue('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: false,
        current_version: '0.2.3',
      });

      await service.checkForUpdates();

      // Second call should skip
      const result = await service.checkForUpdates(false);
      expect(result.available).toBe(false);
      expect(result.currentVersion).toBe('0.2.3');
      // invoke should have been called only once (from first check)
      expect(invoke).toHaveBeenCalledTimes(1);
    });

    it('should force check even if checked recently', async () => {
      vi.mocked(getVersion).mockResolvedValue('0.2.3');
      vi.mocked(invoke).mockResolvedValue({
        available: false,
        current_version: '0.2.3',
      });

      // First check
      await service.checkForUpdates();
      // Forced second check
      await service.checkForUpdates(true);

      expect(invoke).toHaveBeenCalledTimes(2);
    });

    it('should rethrow invoke errors and reset in-progress flag', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockRejectedValueOnce(new Error('Network error'));

      await expect(service.checkForUpdates()).rejects.toThrow('Network error');

      // Should allow another check after error
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: false,
        current_version: '0.2.3',
      });

      const result = await service.checkForUpdates(true);
      expect(result.available).toBe(false);
    });

    it('should map snake_case fields to camelCase', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: true,
        current_version: '0.2.3',
        version: '0.3.0',
        date: '2025-01-01',
        body: 'Notes',
        download_url: 'https://dl.example.com',
        whats_new: ['Item 1'],
      });

      const result = await service.checkForUpdates();
      expect(result.downloadUrl).toBe('https://dl.example.com');
      expect(result.whatsNew).toEqual(['Item 1']);
      expect(result.currentVersion).toBe('0.2.3');
    });
  });

  describe('getCurrentVersion', () => {
    it('should return app version', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('1.2.3');

      const version = await service.getCurrentVersion();
      expect(version).toBe('1.2.3');
    });

    it('should propagate errors', async () => {
      vi.mocked(getVersion).mockRejectedValueOnce(new Error('no version'));
      await expect(service.getCurrentVersion()).rejects.toThrow('no version');
    });
  });

  describe('wasCheckedRecently', () => {
    it('should return false if never checked', () => {
      expect(service.wasCheckedRecently()).toBe(false);
    });

    it('should return true after a recent check', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: false,
        current_version: '0.2.3',
      });

      await service.checkForUpdates();
      expect(service.wasCheckedRecently()).toBe(true);
    });

    it('should return false if check was long ago', async () => {
      vi.mocked(getVersion).mockResolvedValueOnce('0.2.3');
      vi.mocked(invoke).mockResolvedValueOnce({
        available: false,
        current_version: '0.2.3',
      });

      await service.checkForUpdates();

      // Manually manipulate lastCheckTime through a forced old timestamp
      // Access private field via any cast
      (service as any).lastCheckTime = Date.now() - 25 * 60 * 60 * 1000; // 25 hours ago

      expect(service.wasCheckedRecently()).toBe(false);
    });
  });

  describe('singleton export', () => {
    it('should export a singleton instance', () => {
      expect(updateService).toBeInstanceOf(UpdateService);
    });
  });
});

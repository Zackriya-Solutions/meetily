import { beforeEach, describe, expect, it, vi } from 'vitest';

const { mockCheck, mockGetVersion, mockRelaunch } = vi.hoisted(() => ({
  mockCheck: vi.fn(),
  mockGetVersion: vi.fn(),
  mockRelaunch: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@tauri-apps/plugin-process', () => ({
  relaunch: mockRelaunch,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: mockGetVersion,
}));

import { UpdateService } from '@/services/updateService';

describe('UpdateService', () => {
  beforeEach(() => {
    mockCheck.mockReset();
    mockGetVersion.mockReset();
    mockRelaunch.mockReset();
    mockGetVersion.mockResolvedValue('1.0.0');
  });

  it('returns cached update info inside the throttle window', async () => {
    const downloadAndInstall = vi.fn();
    mockCheck.mockResolvedValue({
      available: true,
      version: '1.0.1',
      date: '2026-04-05',
      body: 'Bug fixes',
      downloadAndInstall,
    });

    const service = new UpdateService();

    const first = await service.checkForUpdates();
    const second = await service.checkForUpdates();

    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(second).toEqual(first);
    expect(service.wasCheckedRecently()).toBe(true);
  });

  it('does not mark a failed check as recently completed', async () => {
    mockCheck
      .mockRejectedValueOnce(new Error('network down'))
      .mockResolvedValueOnce({ available: false });

    const service = new UpdateService();

    await expect(service.checkForUpdates()).rejects.toThrow('network down');
    expect(service.wasCheckedRecently()).toBe(false);

    await service.checkForUpdates();
    expect(mockCheck).toHaveBeenCalledTimes(2);
  });

  it('forwards progressive download events and relaunches after install', async () => {
    const downloadAndInstall = vi.fn(async (callback: (event: unknown) => void) => {
      callback({ event: 'Started', data: { contentLength: 100 } });
      callback({ event: 'Progress', data: { chunkLength: 40 } });
      callback({ event: 'Progress', data: { chunkLength: 60 } });
      callback({ event: 'Finished' });
    });

    mockCheck.mockResolvedValue({
      available: true,
      version: '1.0.1',
      date: '2026-04-05',
      body: 'Bug fixes',
      downloadAndInstall,
    });

    const service = new UpdateService();
    await service.checkForUpdates();

    const progress: Array<{ downloaded: number; total: number; percentage: number }> = [];
    await service.downloadAndInstall((update) => {
      progress.push(update);
    });

    expect(progress).toEqual([
      { downloaded: 0, total: 100, percentage: 0 },
      { downloaded: 40, total: 100, percentage: 40 },
      { downloaded: 100, total: 100, percentage: 100 },
      { downloaded: 100, total: 100, percentage: 100 },
    ]);
    expect(mockRelaunch).toHaveBeenCalledTimes(1);
    expect(service.getPendingUpdate()).toBeNull();
  });
});

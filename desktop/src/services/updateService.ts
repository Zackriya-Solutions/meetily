/**
 * Update Service
 *
 * Handles automatic software updates using Tauri updater plugin.
 * Provides update checking, downloading, and installation functionality.
 */

import { check, Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { getVersion } from '@tauri-apps/api/app';

export interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  version?: string;
  date?: string;
  body?: string;
  downloadUrl?: string;
}

export interface UpdateProgress {
  downloaded: number;
  total: number;
  percentage: number;
}

/**
 * Update Service
 * Singleton service for managing app updates
 */
export class UpdateService {
  private updateCheckInProgress = false;
  private lastCheckTime: number | null = null;
  private cachedUpdateInfo: UpdateInfo | null = null;
  private pendingUpdate: Update | null = null;
  private readonly CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 hours

  /**
   * Check for available updates
   * @param force Force check even if recently checked
   * @returns Promise with update information
   */
  async checkForUpdates(force = false): Promise<UpdateInfo> {
    // Prevent concurrent update checks
    if (this.updateCheckInProgress) {
      throw new Error('Update check already in progress');
    }

    // Skip if checked recently (unless forced)
    if (!force && this.lastCheckTime && this.cachedUpdateInfo) {
      const timeSinceLastCheck = Date.now() - this.lastCheckTime;
      if (timeSinceLastCheck < this.CHECK_INTERVAL_MS) {
        console.log('Skipping update check - checked recently');
        return this.cachedUpdateInfo;
      }
    }

    this.updateCheckInProgress = true;

    try {
      const currentVersion = await getVersion();
      const update = await check();
      this.lastCheckTime = Date.now();
      this.pendingUpdate = update?.available ? update : null;

      if (update?.available) {
        const info = {
          available: true,
          currentVersion,
          version: update.version,
          date: update.date,
          body: update.body,
        };
        this.cachedUpdateInfo = info;
        return info;
      }

      const info = {
        available: false,
        currentVersion,
      };
      this.cachedUpdateInfo = info;
      return info;
    } catch (error) {
      console.error('Failed to check for updates:', error);
      throw error;
    } finally {
      this.updateCheckInProgress = false;
    }
  }

  /**
   * Download and install the available update
   * @param update The update object from checkForUpdates
   * @param onProgress Optional progress callback
   * @returns Promise that resolves when download completes
   */
  async downloadAndInstall(
    onProgress?: (progress: UpdateProgress) => void,
    update: Update | null = this.pendingUpdate,
  ): Promise<void> {
    if (!update) {
      throw new Error('No prepared update is available to download');
    }

    try {
      let downloaded = 0;
      let total = 0;

      await update.downloadAndInstall((event) => {
        if (!onProgress) {
          return;
        }

        switch (event.event) {
          case 'Started':
            total = event.data.contentLength ?? 0;
            onProgress({ downloaded: 0, total, percentage: 0 });
            break;
          case 'Progress':
            downloaded += event.data.chunkLength ?? 0;
            onProgress({
              downloaded,
              total,
              percentage: total > 0 ? Math.round((downloaded / total) * 100) : 0,
            });
            break;
          case 'Finished':
            onProgress({ downloaded: total, total, percentage: 100 });
            break;
        }
      });

      this.pendingUpdate = null;
      await relaunch();
    } catch (error) {
      console.error('Failed to download/install update:', error);
      throw error;
    }
  }

  /**
   * Get the current app version
   * @returns Promise with version string
   */
  async getCurrentVersion(): Promise<string> {
    return getVersion();
  }

  /**
   * Check if an update check was performed recently
   * @returns true if checked within the interval
   */
  wasCheckedRecently(): boolean {
    if (!this.lastCheckTime) return false;
    const timeSinceLastCheck = Date.now() - this.lastCheckTime;
    return timeSinceLastCheck < this.CHECK_INTERVAL_MS;
  }

  getPendingUpdate(): Update | null {
    return this.pendingUpdate;
  }
}

// Export singleton instance
export const updateService = new UpdateService();

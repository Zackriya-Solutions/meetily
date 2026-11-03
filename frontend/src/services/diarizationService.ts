/**
 * Diarization Service
 *
 * Handles all speaker diarization (Nemotron-3) backend calls and events.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import {
  DiarizationConfig,
  DiarizationModelInfo,
  DiarizationDownloadProgress,
  DiarizationStreamingPreset,
} from '@/types';

export class DiarizationService {
  /**
   * Initialize the diarization engine in the backend
   */
  async init(): Promise<void> {
    return invoke('diarization_init');
  }

  /**
   * Get metadata and download status of the Nemotron-3 model
   */
  async getModelInfo(): Promise<DiarizationModelInfo> {
    return invoke<DiarizationModelInfo>('diarization_get_model_info');
  }

  /**
   * Check if the model is currently loaded in memory
   */
  async isModelLoaded(): Promise<boolean> {
    return invoke<boolean>('diarization_is_model_loaded');
  }

  /**
   * Check if the model file is downloaded and available on disk
   */
  async isModelAvailable(): Promise<boolean> {
    return invoke<boolean>('diarization_is_model_available');
  }

  /**
   * Load the model into memory for inference
   */
  async loadModel(): Promise<void> {
    return invoke('diarization_load_model');
  }

  /**
   * Unload the model to free system memory
   */
  async unloadModel(): Promise<void> {
    return invoke('diarization_unload_model');
  }

  /**
   * Trigger download of the Nemotron-3 Diarization model (~382 MB)
   */
  async downloadModel(): Promise<void> {
    return invoke('diarization_download_model');
  }

  /**
   * Cancel an in-progress download
   */
  async cancelDownload(): Promise<boolean> {
    return invoke<boolean>('diarization_cancel_download');
  }

  /**
   * Delete the downloaded model file from disk
   */
  async deleteModel(): Promise<void> {
    return invoke('diarization_delete_model');
  }

  /**
   * Get current diarization configuration
   */
  async getSettings(): Promise<DiarizationConfig> {
    return invoke<DiarizationConfig>('diarization_get_settings');
  }

  /**
   * Save diarization configuration
   */
  async saveSettings(
    enabled: boolean,
    preset?: DiarizationStreamingPreset,
    maxSpeakers?: number,
    threshold?: number
  ): Promise<void> {
    return invoke('diarization_save_settings', {
      enabled,
      preset: preset || undefined,
      max_speakers: maxSpeakers,
      maxSpeakers: maxSpeakers,
      threshold,
    });
  }

  /**
   * Check if diarization is enabled and ready to run
   */
  async isReady(): Promise<boolean> {
    return invoke<boolean>('diarization_is_ready');
  }

  /**
   * Open the local folder where diarization models are stored
   */
  async openModelsFolder(): Promise<void> {
    return invoke('open_diarization_models_folder');
  }

  // Event Listeners

  /**
   * Listen for download progress updates
   */
  async onDownloadProgress(
    callback: (progress: DiarizationDownloadProgress) => void
  ): Promise<UnlistenFn> {
    return listen<DiarizationDownloadProgress>(
      'diarization-download-progress',
      (event) => {
        callback(event.payload);
      }
    );
  }

  /**
   * Listen for download completion event
   */
  async onDownloadComplete(
    callback: (payload: { modelName: string; filePath: string }) => void
  ): Promise<UnlistenFn> {
    return listen<{ modelName: string; filePath: string }>(
      'diarization-download-complete',
      (event) => {
        callback(event.payload);
      }
    );
  }
}

export const diarizationService = new DiarizationService();

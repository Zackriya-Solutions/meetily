// Types for Built-in AI (Summary Models) integration
export interface BuiltInModelInfo {
  name: string;
  display_name: string;
  status: BuiltInModelStatus;
  path: string;
  size_mb: number;
  context_size: number;
  description: string;
  gguf_file: string;
}

export type BuiltInModelStatus =
  | { type: 'not_downloaded' }
  | { type: 'downloading', progress: number }
  | { type: 'available' }
  | { type: 'corrupted', file_size: number, expected_min_size: number }
  | { type: 'error', Error: string };

// Helper functions for status handling
export function isModelAvailable(status: BuiltInModelStatus): boolean {
  return status.type === 'available';
}

export function isModelDownloading(status: BuiltInModelStatus): boolean {
  return status.type === 'downloading';
}

export function isModelNotDownloaded(status: BuiltInModelStatus): boolean {
  return status.type === 'not_downloaded';
}

export function isModelCorrupted(status: BuiltInModelStatus): boolean {
  return status.type === 'corrupted';
}

export function isModelError(status: BuiltInModelStatus): boolean {
  return status.type === 'error';
}

export function getStatusColor(status: BuiltInModelStatus): string {
  switch (status.type) {
    case 'available': return 'green';
    case 'downloading': return 'blue';
    case 'not_downloaded': return 'gray';
    case 'corrupted': return 'red';
    case 'error': return 'red';
    default: return 'gray';
  }
}

export function getStatusLabel(status: BuiltInModelStatus): string {
  switch (status.type) {
    case 'available': return 'Available';
    case 'downloading': return `Downloading ${status.progress}%`;
    case 'not_downloaded': return 'Not Downloaded';
    case 'corrupted': return 'Corrupted';
    case 'error': return 'Error';
    default: return 'Unknown';
  }
}

// Tauri command wrappers for Built-in AI backend
import { invoke } from '@tauri-apps/api/core';

export class BuiltInAIAPI {
  static async listModels(): Promise<BuiltInModelInfo[]> {
    return await invoke('builtin_ai_list_models');
  }

  static async getModelInfo(modelName: string): Promise<BuiltInModelInfo | null> {
    return await invoke('builtin_ai_get_model_info', { modelName });
  }

  static async isModelReady(modelName: string, refresh: boolean = false): Promise<boolean> {
    return await invoke('builtin_ai_is_model_ready', { modelName, refresh });
  }

  static async getAvailableModel(): Promise<string | null> {
    return await invoke('builtin_ai_get_available_summary_model');
  }

  static async downloadModel(modelName: string): Promise<void> {
    await invoke('builtin_ai_download_model', { modelName });
  }

  static async cancelDownload(modelName: string): Promise<void> {
    await invoke('builtin_ai_cancel_download', { modelName });
  }

  static async deleteModel(modelName: string): Promise<void> {
    await invoke('builtin_ai_delete_model', { modelName });
  }

  static async getModelsDirectory(): Promise<string> {
    return await invoke('builtin_ai_get_models_directory');
  }
}

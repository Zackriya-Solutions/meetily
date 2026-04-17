// Cohere Transcribe 03-2026 (local ONNX) TypeScript bindings.
//
// The surface mirrors the Tauri commands declared in
// `frontend/src-tauri/src/cohere_engine/commands.rs`. Cohere is the sole
// on-device transcription provider after the 2026 migration.

import { invoke } from '@tauri-apps/api/core';

export type ModelStatus =
  | 'Available'
  | 'Missing'
  | { Downloading: number }
  | { Error: string };

export interface CohereModelInfo {
  name: string;
  path: string;
  size_mb: number;
  status: ModelStatus;
  description?: string;
}

/** Single Cohere model we ship today. Keep in lock-step with DEFAULT_COHERE_MODEL in Rust. */
export const DEFAULT_COHERE_MODEL = 'cohere-transcribe-03-2026';

export const COHERE_MODEL_DISPLAY: Record<string, { title: string; tagline: string; approxSizeGb: string }> = {
  'cohere-transcribe-03-2026': {
    title: 'Cohere Transcribe (03-2026)',
    tagline: '한국어 최적화 로컬 STT · 네트워크 전송 없음',
    approxSizeGb: '약 1.5–2.5 GB',
  },
};

export const CohereAPI = {
  async init(): Promise<void> {
    await invoke('cohere_init');
  },

  async getAvailableModels(): Promise<CohereModelInfo[]> {
    return invoke<CohereModelInfo[]>('cohere_get_available_models');
  },

  async getCurrentModel(): Promise<string | null> {
    return invoke<string | null>('cohere_get_current_model');
  },

  async isModelLoaded(): Promise<boolean> {
    return invoke<boolean>('cohere_is_model_loaded');
  },

  async loadModel(modelName: string): Promise<void> {
    await invoke('cohere_load_model', { modelName });
  },

  async unloadModel(): Promise<void> {
    await invoke('cohere_unload_model');
  },

  async downloadModel(modelName: string): Promise<void> {
    await invoke('cohere_download_model', { modelName });
  },

  async cancelDownload(): Promise<void> {
    await invoke('cohere_cancel_download');
  },

  async validateReady(): Promise<void> {
    await invoke('cohere_validate_model_ready');
  },

  async getModelsDirectory(): Promise<string> {
    return invoke<string>('cohere_get_models_directory');
  },

  async openModelsFolder(): Promise<void> {
    await invoke('open_models_folder');
  },
};

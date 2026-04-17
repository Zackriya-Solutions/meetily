import { useState, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface RawModelInfo {
  name: string;
  size_mb: number;
  status: 'Available' | 'Missing' | { Downloading: number } | { Error: string };
}

export interface ModelOption {
  provider: 'cohere';
  name: string;
  displayName: string;
  size_mb: number;
}

interface TranscriptModelConfig {
  provider?: string;
  model?: string;
}

/**
 * Custom hook for fetching and managing Cohere transcription models.
 */
export function useTranscriptionModels(transcriptModelConfig: TranscriptModelConfig | undefined) {
  const [availableModels, setAvailableModels] = useState<ModelOption[]>([]);
  const [selectedModelKey, setSelectedModelKey] = useState<string>('');
  const [loadingModels, setLoadingModels] = useState(false);
  const userSelectedRef = useRef(false);

  const setSelectedModelKeyWithTracking = useCallback((key: string) => {
    userSelectedRef.current = true;
    setSelectedModelKey(key);
  }, []);

  const fetchModels = useCallback(async () => {
    setLoadingModels(true);
    const allModels: ModelOption[] = [];

    try {
      const cohereModels = await invoke<RawModelInfo[]>('cohere_get_available_models');
      const availableCohere = cohereModels
        .filter((m) => m.status === 'Available')
        .map((m) => ({
          provider: 'cohere' as const,
          name: m.name,
          displayName: `🎙️ Cohere: ${m.name}`,
          size_mb: m.size_mb,
        }));
      allModels.push(...availableCohere);
    } catch (err) {
      console.error('Failed to fetch Cohere models:', err);
    }

    setAvailableModels(allModels);

    const configuredProvider = transcriptModelConfig?.provider || '';
    const configuredModel = transcriptModelConfig?.model || '';

    const configuredMatch = allModels.find(
      (m) => configuredProvider === 'cohere' && m.provider === 'cohere' && m.name === configuredModel
    );

    if (!userSelectedRef.current) {
      if (configuredMatch) {
        setSelectedModelKey(`${configuredMatch.provider}:${configuredMatch.name}`);
      } else if (allModels.length > 0) {
        setSelectedModelKey(`${allModels[0].provider}:${allModels[0].name}`);
      }
    }

    setLoadingModels(false);
  }, [transcriptModelConfig]);

  const resetSelection = useCallback(() => {
    userSelectedRef.current = false;
  }, []);

  return {
    availableModels,
    selectedModelKey,
    setSelectedModelKey: setSelectedModelKeyWithTracking,
    loadingModels,
    fetchModels,
    resetSelection,
  };
}

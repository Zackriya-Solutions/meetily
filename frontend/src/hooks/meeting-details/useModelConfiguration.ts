import { useState, useEffect, useCallback } from 'react';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';
import { apiUrl } from '@/lib/config';
import { authFetch } from '@/lib/api';

interface UseModelConfigurationProps {
  serverAddress: string | null;
}

export function useModelConfiguration({ serverAddress }: UseModelConfigurationProps) {
  // Note: No hardcoded defaults - DB is the source of truth
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'gemini',
    model: 'gemini-2.5-pro', // Default to Gemini
    whisperModel: 'large-v3'
  });
  const [isLoading, setIsLoading] = useState(true);
  const [, setError] = useState<string>('');

  // Fetch model configuration on mount and when serverAddress changes
  useEffect(() => {
    const fetchModelConfig = async () => {
      setIsLoading(true);
      try {
        console.log('🔄 Fetching model configuration from database via HTTP...');
        const response = await authFetch('/get-model-config');
        
        if (response.ok) {
           const data = await response.json();
           if (data && data.provider !== null) {
              console.log('✅ Loaded model config from database:', data);
              setModelConfig(data);
           } else {
              console.warn('⚠️ No model config found in database, using defaults');
           }
        }
      } catch (error) {
        console.error('❌ Failed to fetch model config:', error);
      } finally {
        setIsLoading(false);
        console.log('✅ Model configuration loading complete');
      }
    };

    fetchModelConfig();
  }, [serverAddress]);

  // Listen for model config updates from other components
  useEffect(() => {
    const handleStorageChange = (e: StorageEvent) => {
       if (e.key === 'model_config') {
          // reload if needed
       }
    };
    window.addEventListener('storage', handleStorageChange);
    return () => window.removeEventListener('storage', handleStorageChange);
  }, []);

  // Save model configuration
  const handleSaveModelConfig = useCallback(async (updatedConfig?: ModelConfig) => {
    try {
      const configToSave = updatedConfig || modelConfig;
      const payload = {
        provider: configToSave.provider,
        model: configToSave.model,
        whisperModel: configToSave.whisperModel,
        apiKey: configToSave.apiKey ?? null,
        ollamaEndpoint: configToSave.ollamaEndpoint ?? null
      };
      console.log('Saving model config with payload:', payload);

      // Track model configuration change
      if (updatedConfig && (
        updatedConfig.provider !== modelConfig.provider ||
        updatedConfig.model !== modelConfig.model
      )) {
        await Analytics.trackModelChanged(
          modelConfig.provider,
          modelConfig.model,
          updatedConfig.provider,
          updatedConfig.model
        );
      }

      await authFetch('/save-model-config', {
        method: 'POST',
        // headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });

      console.log('Save model config success');
      setModelConfig(payload);

      toast.success("Summary settings Saved successfully");

      await Analytics.trackSettingsChanged('model_config', `${payload.provider}_${payload.model}`);
    } catch (error) {
      console.error('Failed to save model config:', error);
      toast.error("Failed to save summary settings", { description: String(error) });
      if (error instanceof Error) {
        setError(error.message);
      } else {
        setError('Failed to save model config: Unknown error');
      }
    }
  }, [modelConfig]);

  return {
    modelConfig,
    setModelConfig,
    handleSaveModelConfig,
    isLoading,
  };
}

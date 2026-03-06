'use client';

import { useState, useEffect, useCallback } from 'react';
import { toast } from 'sonner';
import { authFetch } from '@/lib/api';
import { ModelConfig, ModelSettingsModal } from '@/components/ModelSettingsModal';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

interface SummaryModelSettingsProps {
  refetchTrigger?: number; // Change this to trigger refetch
}

export function SummaryModelSettings({ refetchTrigger }: SummaryModelSettingsProps) {
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'gemini',
    model: 'gemini-2.5-pro',
    whisperModel: 'large-v3',
    apiKey: null,
    ollamaEndpoint: null
  });
  const { serverAddress } = useSidebar();

  // Reusable fetch function
  const fetchModelConfig = useCallback(async () => {
    if (!serverAddress) return;
    try {
      const response = await authFetch('/get-model-config');
      if (response.ok) {
        const data = await response.json();
        if (data && data.provider !== null) {
          setModelConfig(data);
        }
      }
    } catch (error) {
      console.error('Failed to fetch model config:', error);
      toast.error('Failed to load model settings');
    }
  }, [serverAddress]);

  // Fetch on mount
  useEffect(() => {
    fetchModelConfig();
  }, [fetchModelConfig]);

  // Refetch when trigger changes (optional external control)
  useEffect(() => {
    if (refetchTrigger !== undefined && refetchTrigger > 0) {
      fetchModelConfig();
    }
  }, [refetchTrigger, fetchModelConfig]);


  // Save handler
  const handleSaveModelConfig = async (config: ModelConfig) => {
    if (!serverAddress) return;
    try {
      const response = await authFetch('/save-model-config', {
        method: 'POST',
        body: JSON.stringify(config)
      });

      if (response.ok) {
        setModelConfig(config);
        toast.success('Model settings saved successfully');
      } else {
        throw new Error('Failed to save');
      }
    } catch (error) {
      console.error('Error saving model config:', error);
      toast.error('Failed to save model settings');
    }
  };

  return (
    <div>
      <h3 className="text-lg font-semibold mb-4">Summary Model Configuration</h3>
      <p className="text-sm text-gray-600 mb-6">
        Configure the AI model used for generating meeting summaries.
      </p>
      <ModelSettingsModal
        modelConfig={modelConfig}
        setModelConfig={setModelConfig}
        onSave={handleSaveModelConfig}
        skipInitialFetch={true}
      />
    </div>
  );
}


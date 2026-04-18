import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { toast } from 'sonner';

export interface ModelConfig {
  provider: 'custom-openai';
  model: string;
  whisperModel: string;
  apiKey?: string | null;
  customOpenAIEndpoint?: string | null;
  customOpenAIModel?: string | null;
  customOpenAIApiKey?: string | null;
  maxTokens?: number | null;
  temperature?: number | null;
  topP?: number | null;
}

interface ModelSettingsModalProps {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSave: (config: ModelConfig) => void;
  skipInitialFetch?: boolean;
}

export function ModelSettingsModal({
  modelConfig,
  setModelConfig,
  onSave,
  skipInitialFetch = false,
}: ModelSettingsModalProps) {
  const [endpoint, setEndpoint] = useState(modelConfig.customOpenAIEndpoint || '');
  const [model, setModel] = useState(modelConfig.customOpenAIModel || modelConfig.model || '');
  const [apiKey, setApiKey] = useState(modelConfig.customOpenAIApiKey || '');
  const [maxTokens, setMaxTokens] = useState(modelConfig.maxTokens?.toString() || '');
  const [temperature, setTemperature] = useState(modelConfig.temperature?.toString() || '');
  const [topP, setTopP] = useState(modelConfig.topP?.toString() || '');
  const [isTestingConnection, setIsTestingConnection] = useState(false);

  useEffect(() => {
    setEndpoint(modelConfig.customOpenAIEndpoint || '');
    setModel(modelConfig.customOpenAIModel || modelConfig.model || '');
    setApiKey(modelConfig.customOpenAIApiKey || '');
    setMaxTokens(modelConfig.maxTokens?.toString() || '');
    setTemperature(modelConfig.temperature?.toString() || '');
    setTopP(modelConfig.topP?.toString() || '');
  }, [modelConfig]);

  useEffect(() => {
    if (skipInitialFetch) {
      return;
    }

    const fetchConfig = async () => {
      try {
        const customConfig = await invoke<any>('api_get_custom_openai_config');
        if (!customConfig) {
          return;
        }

        setEndpoint(customConfig.endpoint || '');
        setModel(customConfig.model || '');
        setApiKey(customConfig.apiKey || '');
        setMaxTokens(customConfig.maxTokens?.toString() || '');
        setTemperature(customConfig.temperature?.toString() || '');
        setTopP(customConfig.topP?.toString() || '');
      } catch (error) {
        console.error('Failed to load custom OpenAI config:', error);
      }
    };

    fetchConfig();
  }, [skipInitialFetch]);

  const isInvalid = !endpoint.trim() || !model.trim();

  const buildConfig = (): ModelConfig => ({
    provider: 'custom-openai',
    model: model.trim(),
    whisperModel: modelConfig.whisperModel || 'large-v3',
    apiKey: null,
    customOpenAIEndpoint: endpoint.trim(),
    customOpenAIModel: model.trim(),
    customOpenAIApiKey: apiKey.trim() || null,
    maxTokens: maxTokens ? parseInt(maxTokens, 10) : null,
    temperature: temperature ? parseFloat(temperature) : null,
    topP: topP ? parseFloat(topP) : null,
  });

  const handleTest = async () => {
    if (isInvalid) {
      toast.error('Endpoint URL and model name are required');
      return;
    }

    setIsTestingConnection(true);
    try {
      const result = await invoke<{ status: string; message: string }>('api_test_custom_openai_connection', {
        endpoint: endpoint.trim(),
        apiKey: apiKey.trim() || null,
        model: model.trim(),
      });
      toast.success(result.message || 'Connection successful');
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setIsTestingConnection(false);
    }
  };

  const handleSave = async () => {
    const nextConfig = buildConfig();

    try {
      await invoke('api_save_custom_openai_config', {
        endpoint: nextConfig.customOpenAIEndpoint,
        apiKey: nextConfig.customOpenAIApiKey,
        model: nextConfig.customOpenAIModel,
        maxTokens: nextConfig.maxTokens,
        temperature: nextConfig.temperature,
        topP: nextConfig.topP,
      });
    } catch (error) {
      console.error('Failed to save custom OpenAI config:', error);
      toast.error('Failed to save custom OpenAI configuration');
      return;
    }

    setModelConfig(nextConfig);
    onSave(nextConfig);
  };

  return (
    <div>
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-lg font-semibold">Model Settings</h3>
      </div>

      <div className="space-y-4">
        <div>
          <Label>Summarization Provider</Label>
          <Input value="Custom OpenAI-compatible endpoint" disabled className="mt-1" />
        </div>

        <div>
          <Label htmlFor="custom-endpoint">Endpoint URL</Label>
          <Input
            id="custom-endpoint"
            value={endpoint}
            onChange={(e) => setEndpoint(e.target.value)}
            placeholder="http://localhost:8000/v1"
            className="mt-1"
          />
        </div>

        <div>
          <Label htmlFor="custom-model">Model Name</Label>
          <Input
            id="custom-model"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="gpt-4o-mini or llama-3.1-8b-instruct"
            className="mt-1"
          />
        </div>

        <div>
          <Label htmlFor="custom-api-key">API Key</Label>
          <Input
            id="custom-api-key"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="Leave empty if not required"
            className="mt-1"
          />
        </div>

        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <div>
            <Label htmlFor="custom-max-tokens">Max Tokens</Label>
            <Input
              id="custom-max-tokens"
              type="number"
              value={maxTokens}
              onChange={(e) => setMaxTokens(e.target.value)}
              placeholder="Optional"
              className="mt-1"
            />
          </div>

          <div>
            <Label htmlFor="custom-temperature">Temperature</Label>
            <Input
              id="custom-temperature"
              type="number"
              step="0.1"
              min="0"
              max="2"
              value={temperature}
              onChange={(e) => setTemperature(e.target.value)}
              placeholder="Optional"
              className="mt-1"
            />
          </div>

          <div>
            <Label htmlFor="custom-top-p">Top P</Label>
            <Input
              id="custom-top-p"
              type="number"
              step="0.1"
              min="0"
              max="1"
              value={topP}
              onChange={(e) => setTopP(e.target.value)}
              placeholder="Optional"
              className="mt-1"
            />
          </div>
        </div>
      </div>

      <div className="mt-6 flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={handleTest} disabled={isTestingConnection || isInvalid}>
          {isTestingConnection ? 'Testing...' : 'Test Connection'}
        </Button>
        <Button type="button" onClick={handleSave} disabled={isInvalid}>
          Save
        </Button>
      </div>
    </div>
  );
}

import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

// Mock the TranscriptSettings import used by configService
vi.mock('@/components/TranscriptSettings', () => ({
  // Just need the type to exist - no runtime value needed
}));

import { ConfigService, configService } from './configService';

describe('ConfigService', () => {
  let service: ConfigService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new ConfigService();
  });

  describe('getTranscriptConfig', () => {
    it('should invoke api_get_transcript_config and return result', async () => {
      const config = { provider: 'ollama', model: 'llama3' };
      vi.mocked(invoke).mockResolvedValueOnce(config);

      const result = await service.getTranscriptConfig();
      expect(result).toEqual(config);
      expect(invoke).toHaveBeenCalledWith('api_get_transcript_config');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('config not found'));
      await expect(service.getTranscriptConfig()).rejects.toThrow('config not found');
    });
  });

  describe('getModelConfig', () => {
    it('should invoke api_get_model_config and return result', async () => {
      const config = { provider: 'groq', model: 'whisper-large', whisperModel: 'large-v3' };
      vi.mocked(invoke).mockResolvedValueOnce(config);

      const result = await service.getModelConfig();
      expect(result).toEqual(config);
      expect(invoke).toHaveBeenCalledWith('api_get_model_config');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('backend error'));
      await expect(service.getModelConfig()).rejects.toThrow('backend error');
    });
  });

  describe('getRecordingPreferences', () => {
    it('should invoke get_recording_preferences and return result', async () => {
      const prefs = { preferred_mic_device: 'Mic A', preferred_system_device: null };
      vi.mocked(invoke).mockResolvedValueOnce(prefs);

      const result = await service.getRecordingPreferences();
      expect(result).toEqual(prefs);
      expect(invoke).toHaveBeenCalledWith('get_recording_preferences');
    });

    it('should handle null device preferences', async () => {
      const prefs = { preferred_mic_device: null, preferred_system_device: null };
      vi.mocked(invoke).mockResolvedValueOnce(prefs);

      const result = await service.getRecordingPreferences();
      expect(result.preferred_mic_device).toBeNull();
      expect(result.preferred_system_device).toBeNull();
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'));
      await expect(service.getRecordingPreferences()).rejects.toThrow('fail');
    });
  });

  describe('getCustomOpenAIConfig', () => {
    it('should invoke api_get_custom_openai_config and return config', async () => {
      const config = {
        endpoint: 'https://api.custom.com/v1',
        apiKey: 'key-123',
        model: 'gpt-4',
        maxTokens: 2048,
        temperature: 0.7,
        topP: 0.9,
      };
      vi.mocked(invoke).mockResolvedValueOnce(config);

      const result = await service.getCustomOpenAIConfig();
      expect(result).toEqual(config);
      expect(invoke).toHaveBeenCalledWith('api_get_custom_openai_config');
    });

    it('should return null when not configured', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(null);

      const result = await service.getCustomOpenAIConfig();
      expect(result).toBeNull();
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'));
      await expect(service.getCustomOpenAIConfig()).rejects.toThrow('fail');
    });
  });

  describe('saveCustomOpenAIConfig', () => {
    it('should invoke api_save_custom_openai_config with correct params', async () => {
      const config = {
        endpoint: 'https://api.custom.com/v1',
        apiKey: 'key-123',
        model: 'gpt-4',
        maxTokens: 2048,
        temperature: 0.7,
        topP: 0.9,
      };
      const response = { status: 'ok', message: 'Saved' };
      vi.mocked(invoke).mockResolvedValueOnce(response);

      const result = await service.saveCustomOpenAIConfig(config);
      expect(result).toEqual(response);
      expect(invoke).toHaveBeenCalledWith('api_save_custom_openai_config', {
        endpoint: config.endpoint,
        apiKey: config.apiKey,
        model: config.model,
        maxTokens: config.maxTokens,
        temperature: config.temperature,
        topP: config.topP,
      });
    });

    it('should handle null optional fields', async () => {
      const config = {
        endpoint: 'https://api.custom.com/v1',
        apiKey: null,
        model: 'gpt-4',
        maxTokens: null,
        temperature: null,
        topP: null,
      };
      vi.mocked(invoke).mockResolvedValueOnce({ status: 'ok', message: 'Saved' });

      await service.saveCustomOpenAIConfig(config);
      expect(invoke).toHaveBeenCalledWith('api_save_custom_openai_config', {
        endpoint: config.endpoint,
        apiKey: null,
        model: config.model,
        maxTokens: null,
        temperature: null,
        topP: null,
      });
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('save fail'));
      await expect(service.saveCustomOpenAIConfig({
        endpoint: 'x', apiKey: null, model: 'x', maxTokens: null, temperature: null, topP: null,
      })).rejects.toThrow('save fail');
    });
  });

  describe('testCustomOpenAIConnection', () => {
    it('should invoke api_test_custom_openai_connection', async () => {
      const response = { status: 'ok', message: 'Connected', http_status: 200 };
      vi.mocked(invoke).mockResolvedValueOnce(response);

      const result = await service.testCustomOpenAIConnection('https://api.com/v1', 'key', 'gpt-4');
      expect(result).toEqual(response);
      expect(invoke).toHaveBeenCalledWith('api_test_custom_openai_connection', {
        endpoint: 'https://api.com/v1',
        apiKey: 'key',
        model: 'gpt-4',
      });
    });

    it('should handle null apiKey', async () => {
      vi.mocked(invoke).mockResolvedValueOnce({ status: 'ok', message: 'Connected' });

      await service.testCustomOpenAIConnection('https://api.com/v1', null, 'model-x');
      expect(invoke).toHaveBeenCalledWith('api_test_custom_openai_connection', {
        endpoint: 'https://api.com/v1',
        apiKey: null,
        model: 'model-x',
      });
    });

    it('should return error status', async () => {
      const response = { status: 'error', message: 'Connection refused', http_status: 502 };
      vi.mocked(invoke).mockResolvedValueOnce(response);

      const result = await service.testCustomOpenAIConnection('https://bad.com', null, 'x');
      expect(result.status).toBe('error');
      expect(result.http_status).toBe(502);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('timeout'));
      await expect(service.testCustomOpenAIConnection('x', null, 'x'))
        .rejects.toThrow('timeout');
    });
  });

  describe('singleton export', () => {
    it('should export a singleton instance', () => {
      expect(configService).toBeInstanceOf(ConfigService);
    });
  });
});

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { ConfigProvider, useConfig } from './ConfigContext';
import { invoke } from '@tauri-apps/api/core';

// Ensure window.__TAURI_INTERNALS__ exists for dynamic imports of @tauri-apps/api/event
// ConfigContext uses `await import('@tauri-apps/api/event')` which bypasses vi.mock
// when the actual module tries to access Tauri internals
beforeEach(() => {
  (globalThis as any).window = globalThis.window || {};
  (window as any).__TAURI_INTERNALS__ = {
    transformCallback: vi.fn().mockReturnValue(0),
    invoke: vi.fn().mockResolvedValue(undefined),
  };
});

// Mock configService
vi.mock('@/services/configService', () => ({
  configService: {
    getTranscriptConfig: vi.fn().mockResolvedValue({
      provider: 'parakeet',
      model: 'parakeet-tdt-0.6b-v3-int8',
      apiKey: null,
    }),
    getModelConfig: vi.fn().mockResolvedValue({
      provider: 'ollama',
      model: 'llama3.2:latest',
      whisperModel: 'large-v3',
      ollamaEndpoint: null,
    }),
    getRecordingPreferences: vi.fn().mockResolvedValue({
      preferred_mic_device: null,
      preferred_system_device: null,
    }),
    getCustomOpenAIConfig: vi.fn().mockResolvedValue(null),
  },
  ConfigService: vi.fn(),
  ModelConfig: {},
}));

// Mock TranscriptSettings import
vi.mock('@/components/TranscriptSettings', () => ({
  TranscriptModelProps: {},
}));

// Mock DeviceSelection import
vi.mock('@/components/DeviceSelection', () => ({
  SelectedDevices: {},
}));

const mockInvoke = vi.mocked(invoke);

// Helper consumer component
function TestConsumer() {
  const config = useConfig();
  return (
    <div>
      <span data-testid="provider">{config.modelConfig.provider}</span>
      <span data-testid="model">{config.modelConfig.model}</span>
      <span data-testid="whisperModel">{config.modelConfig.whisperModel}</span>
      <span data-testid="language">{config.selectedLanguage}</span>
      <span data-testid="confidence">{String(config.showConfidenceIndicator)}</span>
      <span data-testid="autoSummary">{String(config.isAutoSummary)}</span>
      <span data-testid="error">{config.error || 'none'}</span>
      <span data-testid="claudeKey">{config.providerApiKeys.claude || 'null'}</span>
      <span data-testid="groqKey">{config.providerApiKeys.groq || 'null'}</span>
      <span data-testid="loadingPrefs">{String(config.isLoadingPreferences)}</span>
      <span data-testid="transcriptProvider">{config.transcriptModelConfig.provider}</span>
      <button onClick={() => config.setSelectedLanguage('en')}>SetLang</button>
      <button onClick={() => config.toggleConfidenceIndicator(false)}>ToggleConfidence</button>
      <button onClick={() => config.toggleIsAutoSummary(true)}>ToggleAutoSummary</button>
      <button onClick={() => config.updateProviderApiKey('claude', 'key_abc')}>SetClaudeKey</button>
      <button onClick={() => config.setModelConfig({ ...config.modelConfig, provider: 'groq', model: 'llama-3.3-70b-versatile' })}>SetGroq</button>
      <button onClick={() => config.loadPreferences()}>LoadPrefs</button>
    </div>
  );
}

function renderWithProvider() {
  return render(
    <ConfigProvider>
      <TestConsumer />
    </ConfigProvider>
  );
}

describe('ConfigContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default mock for get_ollama_models
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_ollama_models') return [];
      if (cmd === 'api_get_api_key') return null;
      if (cmd === 'get_notification_settings') return null;
      if (cmd === 'get_database_directory') return '/db';
      if (cmd === 'whisper_get_models_directory') return '/models';
      if (cmd === 'get_default_recordings_folder_path') return '/recordings';
      if (cmd === 'set_notification_settings') return undefined;
      throw new Error(`invoke not mocked for: ${cmd}`);
    });
  });

  describe('useConfig hook', () => {
    it('throws when used outside of ConfigProvider', () => {
      const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
      expect(() => render(<TestConsumer />)).toThrow('useConfig must be used within a ConfigProvider');
      spy.mockRestore();
    });
  });

  describe('initial state', () => {
    it('provides default model config values', async () => {
      renderWithProvider();
      // Default values before config loads
      expect(screen.getByTestId('provider').textContent).toBe('ollama');
      expect(screen.getByTestId('model').textContent).toBe('llama3.2:latest');
      expect(screen.getByTestId('whisperModel').textContent).toBe('large-v3');
    });

    it('provides default language as auto', () => {
      renderWithProvider();
      expect(screen.getByTestId('language').textContent).toBe('auto');
    });

    it('provides default confidence indicator as true', () => {
      renderWithProvider();
      expect(screen.getByTestId('confidence').textContent).toBe('true');
    });

    it('provides default autoSummary as false', () => {
      renderWithProvider();
      expect(screen.getByTestId('autoSummary').textContent).toBe('false');
    });

    it('provides default transcript provider as parakeet', () => {
      renderWithProvider();
      expect(screen.getByTestId('transcriptProvider').textContent).toBe('parakeet');
    });
  });

  describe('model config loading', () => {
    it('loads Ollama models on mount', async () => {
      renderWithProvider();
      await waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith('get_ollama_models', expect.any(Object));
      });
    });

    it('handles Ollama model load error', async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === 'get_ollama_models') throw new Error('Connection refused');
        if (cmd === 'api_get_api_key') return null;
        throw new Error(`invoke not mocked: ${cmd}`);
      });
      const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('error').textContent).not.toBe('none');
      });
      spy.mockRestore();
    });
  });

  describe('provider API keys', () => {
    it('loads all provider API keys on mount', async () => {
      mockInvoke.mockImplementation(async (cmd: string, args: any) => {
        if (cmd === 'api_get_api_key') {
          if (args?.provider === 'claude') return 'claude_key';
          if (args?.provider === 'groq') return 'groq_key';
          return null;
        }
        if (cmd === 'get_ollama_models') return [];
        throw new Error(`invoke not mocked: ${cmd}`);
      });
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('claudeKey').textContent).toBe('claude_key');
      });
      await waitFor(() => {
        expect(screen.getByTestId('groqKey').textContent).toBe('groq_key');
      });
    });

    it('updates individual provider key', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('claudeKey').textContent).toBe('null'));

      await user.click(screen.getByText('SetClaudeKey'));
      expect(screen.getByTestId('claudeKey').textContent).toBe('key_abc');
    });
  });

  describe('user interactions', () => {
    it('sets language preference', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      await user.click(screen.getByText('SetLang'));
      expect(screen.getByTestId('language').textContent).toBe('en');
    });

    it('toggles confidence indicator and persists to localStorage', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      expect(screen.getByTestId('confidence').textContent).toBe('true');

      await user.click(screen.getByText('ToggleConfidence'));
      expect(screen.getByTestId('confidence').textContent).toBe('false');
      expect(localStorage.setItem).toHaveBeenCalledWith('showConfidenceIndicator', 'false');
    });

    it('toggles auto summary and persists to localStorage', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      expect(screen.getByTestId('autoSummary').textContent).toBe('false');

      await user.click(screen.getByText('ToggleAutoSummary'));
      expect(screen.getByTestId('autoSummary').textContent).toBe('true');
      expect(localStorage.setItem).toHaveBeenCalledWith('isAutoSummary', 'true');
    });

    it('updates model config', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      await user.click(screen.getByText('SetGroq'));
      expect(screen.getByTestId('provider').textContent).toBe('groq');
      expect(screen.getByTestId('model').textContent).toBe('llama-3.3-70b-versatile');
    });
  });

  describe('preferences loading', () => {
    it('loads preferences lazily when requested', async () => {
      const user = userEvent.setup();
      renderWithProvider();
      expect(screen.getByTestId('loadingPrefs').textContent).toBe('false');

      await user.click(screen.getByText('LoadPrefs'));

      await waitFor(() => {
        expect(screen.getByTestId('loadingPrefs').textContent).toBe('false');
      });
      expect(mockInvoke).toHaveBeenCalledWith('get_database_directory');
      expect(mockInvoke).toHaveBeenCalledWith('whisper_get_models_directory');
      expect(mockInvoke).toHaveBeenCalledWith('get_default_recordings_folder_path');
    });

    it('only loads preferences once (caches result)', async () => {
      const user = userEvent.setup();
      renderWithProvider();

      await user.click(screen.getByText('LoadPrefs'));
      await waitFor(() => expect(screen.getByTestId('loadingPrefs').textContent).toBe('false'));

      const callCount = mockInvoke.mock.calls.filter(c => c[0] === 'get_database_directory').length;

      await user.click(screen.getByText('LoadPrefs'));
      await waitFor(() => expect(screen.getByTestId('loadingPrefs').textContent).toBe('false'));

      const callCountAfter = mockInvoke.mock.calls.filter(c => c[0] === 'get_database_directory').length;
      expect(callCountAfter).toBe(callCount); // No additional calls
    });
  });
});

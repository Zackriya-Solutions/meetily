'use client';

import React, { createContext, useContext, useState, useEffect, useCallback, useMemo, ReactNode, useRef } from 'react';
import { TranscriptModelProps } from '@/components/TranscriptSettings';
import { SelectedDevices } from '@/components/DeviceSelection';
import { configService, ModelConfig } from '@/services/configService';
import { invoke } from '@tauri-apps/api/core';
import Analytics from '@/lib/analytics';
import { BetaFeatures, BetaFeatureKey, loadBetaFeatures, saveBetaFeatures } from '@/types/betaFeatures';

export interface StorageLocations {
  database: string;
  models: string;
  recordings: string;
}

export interface NotificationSettings {
  recording_notifications: boolean;
  time_based_reminders: boolean;
  meeting_reminders: boolean;
  respect_do_not_disturb: boolean;
  notification_sound: boolean;
  system_permission_granted: boolean;
  consent_given: boolean;
  manual_dnd_mode: boolean;
  notification_preferences: {
    show_recording_started: boolean;
    show_recording_stopped: boolean;
    show_recording_paused: boolean;
    show_recording_resumed: boolean;
    show_transcription_complete: boolean;
    show_meeting_reminders: boolean;
    show_system_errors: boolean;
    meeting_reminder_minutes: number[];
  };
}

interface ConfigContextType {
  // Model configuration
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;

  // Transcript model configuration
  transcriptModelConfig: TranscriptModelProps;
  setTranscriptModelConfig: (config: TranscriptModelProps | ((prev: TranscriptModelProps) => TranscriptModelProps)) => void;

  // Device configuration
  selectedDevices: SelectedDevices;
  setSelectedDevices: (devices: SelectedDevices) => void;

  // Language preference
  selectedLanguage: string;
  setSelectedLanguage: (lang: string) => void;

  // UI preferences
  showConfidenceIndicator: boolean;
  toggleConfidenceIndicator: (checked: boolean) => void;

  // Beta features
  betaFeatures: BetaFeatures;
  toggleBetaFeature: (featureKey: BetaFeatureKey, enabled: boolean) => void;

  models: string[];
  modelOptions: Record<ModelConfig['provider'], string[]>;
  error: string;

  // Summary configuration
  isAutoSummary: boolean;
  toggleIsAutoSummary: (checked: boolean) => void;

  // Preference settings (lazy loaded)
  notificationSettings: NotificationSettings | null;
  storageLocations: StorageLocations | null;
  isLoadingPreferences: boolean;
  loadPreferences: () => Promise<void>;
  updateNotificationSettings: (settings: NotificationSettings) => Promise<void>;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);


export function ConfigProvider({ children }: { children: ReactNode }) {
  // Model configuration state
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'custom-openai',
    model: '',
    whisperModel: 'large-v3',
    customOpenAIEndpoint: null,
    customOpenAIModel: null,
    customOpenAIApiKey: null,
  });

  // Transcript model configuration state
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'parakeet',
    model: 'parakeet-tdt-0.6b-v3-int8',
    apiKey: null
  });

  const [models] = useState<string[]>([]);
  const [error, setError] = useState<string>('');

  // Device configuration state
  const [selectedDevices, setSelectedDevices] = useState<SelectedDevices>({
    micDevice: null,
    systemDevice: null
  });

  // Language preference state
  const [selectedLanguage, setSelectedLanguage] = useState<string>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('primaryLanguage');
      return saved || 'auto';
    }
    return 'auto';
  });

  // UI preferences state
  const [showConfidenceIndicator, setShowConfidenceIndicator] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('showConfidenceIndicator');
      return saved !== null ? saved === 'true' : true;
    }
    return true;
  });

  // Summary configs
  const [isAutoSummary, setisAutoSummary] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('isAutoSummary');
      return saved !== null ? saved === 'true' : false
    }
    return false;
  });

  // Beta features state (localStorage)
  const [betaFeatures, setBetaFeatures] = useState<BetaFeatures>(() => {
    return loadBetaFeatures();
  });

  // Preference settings state (lazy loaded)
  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings | null>(null);
  const [storageLocations, setStorageLocations] = useState<StorageLocations | null>(null);
  const [isLoadingPreferences, setIsLoadingPreferences] = useState(false);
  const preferencesLoadedRef = useRef(false);
  const isLoadingRef = useRef(false);

  // Load transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const config = await configService.getTranscriptConfig();
        if (config) {
          console.log('[ConfigContext] Loaded saved transcript config:', config);
          setTranscriptModelConfig({
            provider: config.provider || 'parakeet',
            model: config.model || 'parakeet-tdt-0.6b-v3-int8',
            apiKey: config.apiKey || null
          });
        }
      } catch (error) {
        console.error('[ConfigContext] Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, []);

  // Sync language preference to Rust on mount (fixes startup desync bug)
  useEffect(() => {
    if (selectedLanguage) {
      invoke('set_language_preference', { language: selectedLanguage })
        .then(() => {
          console.log('[ConfigContext] Synced language preference to Rust on startup:', selectedLanguage);
        })
        .catch(err => {
          console.error('[ConfigContext] Failed to sync language preference to Rust on startup:', err);
        });
    }
  }, []); 

  // Load model configuration on mount
  useEffect(() => {
    const fetchModelConfig = async () => {
      try {
        const data = await configService.getModelConfig();
        if (data && data.provider) {
          if (data.provider === 'custom-openai') {
            try {
              const customConfig = await configService.getCustomOpenAIConfig();
              if (customConfig) {
                // Merge custom config fields into modelConfig
                console.log('[ConfigContext] Loading custom OpenAI config:', {
                  endpoint: customConfig.endpoint,
                  model: customConfig.model,
                });
                const resolvedModel = customConfig.model || data.model || '';
                setModelConfig(prev => ({
                  ...prev,
                  provider: data.provider,
                  model: resolvedModel || prev.model,
                  whisperModel: data.whisperModel || prev.whisperModel,
                  customOpenAIEndpoint: customConfig.endpoint,
                  customOpenAIModel: customConfig.model,
                  customOpenAIApiKey: customConfig.apiKey,
                  maxTokens: customConfig.maxTokens,
                  temperature: customConfig.temperature,
                  topP: customConfig.topP,
                }));

                // Seed per-provider model cache from DB
                if (resolvedModel) {
                  const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
                  map[data.provider] = resolvedModel;
                  localStorage.setItem('providerModelMap', JSON.stringify(map));
                }

                return; // Early return
              }
            } catch (err) {
              console.error('[ConfigContext] Failed to fetch custom OpenAI config:', err);
            }
          }

          setModelConfig(prev => ({
            ...prev,
            provider: data.provider,
            model: data.model || prev.model,
            whisperModel: data.whisperModel || prev.whisperModel,
          }));

          // Seed per-provider model cache from DB
          if (data.model) {
            const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
            map[data.provider] = data.model;
            localStorage.setItem('providerModelMap', JSON.stringify(map));
          }
        }
      } catch (error) {
        console.error('Failed to fetch saved model config in ConfigContext:', error);
      }
    };
    fetchModelConfig();
  }, []);

  // Listen for model config updates from other components
  useEffect(() => {
    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen<ModelConfig>('model-config-updated', (event) => {
        console.log('[ConfigContext] Received model-config-updated event:', event.payload);
        setModelConfig(event.payload);
      });
      return unlisten;
    };

    let cleanup: (() => void) | undefined;
    setupListener().then(fn => cleanup = fn);

    return () => {
      cleanup?.();
    };
  }, []);

  // Load device preferences on mount
  useEffect(() => {
    const loadDevicePreferences = async () => {
      try {
        const prefs = await configService.getRecordingPreferences();
        if (prefs && (prefs.preferred_mic_device || prefs.preferred_system_device)) {
          setSelectedDevices({
            micDevice: prefs.preferred_mic_device,
            systemDevice: prefs.preferred_system_device
          });
          console.log('Loaded device preferences:', prefs);
        }
      } catch (error) {
        console.log('No device preferences found or failed to load:', error);
      }
    };
    loadDevicePreferences();
  }, []);

  // Calculate model options based on available models
  const modelOptions: Record<ModelConfig['provider'], string[]> = {
    'custom-openai': modelConfig.model ? [modelConfig.model] : [],
  };

  // Toggle confidence indicator with localStorage persistence
  const toggleConfidenceIndicator = useCallback((checked: boolean) => {
    setShowConfidenceIndicator(checked);
    if (typeof window !== 'undefined') {
      localStorage.setItem('showConfidenceIndicator', checked.toString());
    }
    // Trigger a custom event to notify other components
    window.dispatchEvent(new CustomEvent('confidenceIndicatorChanged', { detail: checked }));
  }, []);

  const toggleIsAutoSummary = useCallback((checked: boolean) => {
    setisAutoSummary(checked);
    if (typeof window !== 'undefined') {
      localStorage.setItem('isAutoSummary', checked.toString());
    }
  }, [])

  // Toggle beta feature with localStorage persistence and analytics
  const toggleBetaFeature = useCallback((featureKey: BetaFeatureKey, enabled: boolean) => {
    setBetaFeatures(prev => {
      const updated = { ...prev, [featureKey]: enabled };
      saveBetaFeatures(updated);

      // Track analytics with specific feature
      Analytics.track('beta_feature_toggled', {
        feature: featureKey,
        enabled: enabled.toString(),
      }).catch(err => console.error('Failed to track beta feature toggle:', err));

      return updated;
    });
  }, []);

  // Lazy load preference settings (only loads if not already cached)
  const loadPreferences = useCallback(async () => {
    // If already loaded, don't reload
    if (preferencesLoadedRef.current) {
      return;
    }

    // If currently loading, don't start another load
    if (isLoadingRef.current) {
      return;
    }

    isLoadingRef.current = true;
    setIsLoadingPreferences(true);
    try {
      // Load notification settings from backend
      let settings: NotificationSettings | null = null;
      try {
        settings = await invoke<NotificationSettings>('get_notification_settings');
        setNotificationSettings(settings);
      } catch (notifError) {
        console.error('[ConfigContext] Failed to load notification settings:', notifError);
        // Use default values if notification settings fail to load
        setNotificationSettings(null);
      }

      // Load storage locations
      const [dbDir, modelsDir, recordingsDir] = await Promise.all([
        invoke<string>('get_database_directory'),
        invoke<string>('whisper_get_models_directory'),
        invoke<string>('get_default_recordings_folder_path')
      ]);

      setStorageLocations({
        database: dbDir,
        models: modelsDir,
        recordings: recordingsDir
      });

      // Mark as loaded
      preferencesLoadedRef.current = true;
    } catch (error) {
      console.error('[ConfigContext] Failed to load preferences:', error);
    } finally {
      isLoadingRef.current = false;
      setIsLoadingPreferences(false);
    }
  }, []);

  // Update notification settings
  const updateNotificationSettings = useCallback(async (settings: NotificationSettings) => {
    try {
      await invoke('set_notification_settings', { settings });
      setNotificationSettings(settings);
    } catch (error) {
      console.error('[ConfigContext] Failed to update notification settings:', error);
      throw error; // Re-throw so component can handle error
    }
  }, []);

  // Wrapper for setSelectedLanguage that persists to localStorage and syncs to Rust
  const handleSetSelectedLanguage = useCallback((lang: string) => {
    setSelectedLanguage(lang);
    if (typeof window !== 'undefined') {
      localStorage.setItem('primaryLanguage', lang);
    }
    // Sync with Rust in-memory state for live recording
    invoke('set_language_preference', { language: lang }).catch(err =>
      console.error('Failed to sync language preference to Rust:', err)
    );
  }, []);

  const value: ConfigContextType = useMemo(() => ({
    modelConfig,
    setModelConfig,
    isAutoSummary,
    toggleIsAutoSummary,
    transcriptModelConfig,
    setTranscriptModelConfig,
    selectedDevices,
    setSelectedDevices,
    selectedLanguage,
    setSelectedLanguage: handleSetSelectedLanguage,
    showConfidenceIndicator,
    toggleConfidenceIndicator,
    betaFeatures,
    toggleBetaFeature,
    models,
    modelOptions,
    error,
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings,
  }), [
    modelConfig,
    isAutoSummary,
    toggleIsAutoSummary,
    transcriptModelConfig,
    selectedDevices,
    selectedLanguage,
    handleSetSelectedLanguage,
    showConfidenceIndicator,
    toggleConfidenceIndicator,
    betaFeatures,
    toggleBetaFeature,
    models,
    modelOptions,
    error,
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings,
  ]);

  return (
    <ConfigContext.Provider value={value}>
      {children}
    </ConfigContext.Provider>
  );
}

export function useConfig() {
  const context = useContext(ConfigContext);
  if (context === undefined) {
    throw new Error('useConfig must be used within a ConfigProvider');
  }
  return context;
}

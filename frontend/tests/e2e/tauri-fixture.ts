import type { Page } from '@playwright/test';

export async function mockTauri(page: Page, onboardingCompleted = true, onboardingStep = 1) {
  page.on('pageerror', error => console.error('PAGEERROR', error.message));
  await page.addInitScript(({ onboardingCompleted, onboardingStep }) => {
    const stores = new Map<string, unknown>();
    let callback = 0;
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners = new Map<string, number[]>();
    const commands: string[] = [];
    Object.assign(window, {
      __themeTestCommands: commands,
      __themeTestEmit: (event: string, payload: unknown) => {
        for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, id, payload });
      },
      __TAURI_OS_PLUGIN_INTERNALS__: { platform: 'macos', family: 'unix', type: 'macos', version: '15.0', arch: 'aarch64', eol: '\n' },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
      __TAURI_INTERNALS__: {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback: (fn: (event: unknown) => void) => { callbacks.set(++callback, fn); return callback; },
        unregisterCallback: () => {},
        invoke: async (cmd: string, args: Record<string, any> = {}) => {
          commands.push(cmd);
          if (cmd === 'plugin:store|load') return args.path;
          if (cmd === 'plugin:store|has') return stores.has(`${args.rid}:${args.key}`);
          if (cmd === 'plugin:store|get') {
            const key = `${args.rid}:${args.key}`;
            return [stores.get(key), stores.has(key)];
          }
          if (cmd === 'plugin:store|set') { stores.set(`${args.rid}:${args.key}`, args.value); return; }
          if (cmd === 'plugin:store|save' || cmd === 'plugin:event|unlisten' || cmd === 'plugin:window|set_theme' || cmd === 'set_language_preference') return;
          if (cmd === 'plugin:event|listen') { listeners.set(args.event, [...(listeners.get(args.event) || []), args.handler]); return args.handler; }
          if (cmd === 'plugin:app|version') return '0.4.1';
          if (cmd === 'plugin:path|resolve_directory') return '/tmp/meetily-test';
          if (cmd === 'plugin:updater|check') return null;
          if (cmd === 'get_onboarding_status') return { completed: onboardingCompleted, current_step: onboardingStep, model_status: { parakeet: 'not_downloaded', summary: 'not_downloaded', selected_summary_model: 'gemma3:1b' } };
          if (cmd === 'parakeet_init' || cmd === 'save_onboarding_status_cmd') return;
          if (cmd === 'parakeet_has_available_models' || cmd === 'builtin_ai_is_model_ready') return false;
          if (cmd === 'builtin_ai_get_recommended_model') return 'gemma3:1b';
          if (cmd === 'parakeet_get_available_models' || cmd === 'builtin_ai_list_models') return [];
          if (cmd === 'parakeet_download_model' || cmd === 'builtin_ai_download_model') return new Promise(() => {});
          if (cmd === 'check_first_launch') return !onboardingCompleted;
          if (cmd === 'get_audio_devices') return [{ name: 'Test microphone', device_type: 'Input' }, { name: 'Test speakers', device_type: 'Output' }];
          if (cmd === 'get_recording_state') return { is_recording: false, is_paused: false, is_active: false, recording_duration: null, active_duration: null };
          if (cmd === 'is_recording' || cmd === 'is_analytics_enabled') return false;
          if (cmd === 'api_get_transcript_config') return { provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8', apiKey: null };
          if (cmd === 'api_get_model_config') return { provider: 'ollama', model: 'llama3.2:latest', whisperModel: 'large-v3' };
          if (cmd === 'api_get_api_key' || cmd === 'api_get_custom_openai_config' || cmd === 'get_recording_meeting_name') return null;
          if (cmd === 'api_get_meeting_metadata') return { id: args.meetingId, title: 'Theme proof meeting', created_at: '2026-11-02T10:00:00Z', updated_at: '2026-11-02T10:00:00Z' };
          if (cmd === 'api_get_meeting_transcripts') return { transcripts: [{ id: 'transcript-1', text: 'Review the launch plan.', timestamp: '10:00:00', audio_start_time: 0, audio_end_time: 3 }], total_count: 1, has_more: false };
          if (cmd === 'api_get_summary') return { status: 'completed', meeting_id: args.meetingId, data: args.meetingId === 'markdown' ? { markdown: '# Decisions\n\nShip dark mode with system appearance.' } : { summary_json: [{ id: 'summary-1', type: 'paragraph', content: [{ type: 'text', text: 'Ship dark mode with system appearance.', styles: {} }], children: [] }] } };
          if (cmd === 'api_list_templates') return [];
          if (cmd === 'api_get_meeting_summary_language') return null;
          if (cmd === 'api_get_meetings'  || cmd === 'get_ollama_models' || cmd === 'get_unprocessed_meetings') return [];
          if (cmd === 'get_recording_preferences') return { auto_save: true, file_format: 'mp4', save_folder: '/tmp/meetily-test/recordings', preferred_mic_device: null, preferred_system_device: null };
          if (cmd === 'get_notification_settings') return { notification_preferences: { show_recording_started: true, show_recording_stopped: true } };
          if (['get_database_directory', 'whisper_get_models_directory', 'get_default_recordings_folder_path'].includes(cmd)) return '/tmp/meetily-test/recordings';
          throw new Error(`Unmocked Tauri command: ${cmd}`);
        },
      },
    });
  }, { onboardingCompleted, onboardingStep });
}

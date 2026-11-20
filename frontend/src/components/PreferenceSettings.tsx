"use client"

import { useEffect, useState, useRef } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen, Save, X } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import Analytics from "@/lib/analytics"
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"
import { OutlookCalendarSettings } from "./OutlookCalendarSettings"
import { usePlatform } from "@/hooks/usePlatform"
import { toast } from "sonner"

const isTauriRuntime = () => (
  typeof window !== 'undefined' &&
  '__TAURI_INTERNALS__' in window
);

/** Mirrors `PipelineSettings` in src-tauri/src/pipeline/settings.rs. */
interface PipelineSettings {
  enabled: boolean;
  scan_interval_minutes: number;
  idle_minutes: number;
  grace_minutes: number;
  summary_template_id: string;
  summary_chunk_size: number;
  summary_overlap: number;
  diarization: boolean;
  max_attempts: number;
  run_state: unknown;
  last_scan_at: string | null;
}

/** Mirrors `ObsidianSettingsView` in src-tauri/src/obsidian.rs. */
interface ObsidianSettingsView {
  vault_path: string | null;
  filename_template: string;
  auto_export: boolean;
  /** Applied when `filename_template` is empty. Shown as the placeholder so
   *  this component never hardcodes a template of its own. */
  default_filename_template: string;
}

export function PreferenceSettings() {
  const platform = usePlatform();
  const {
    notificationSettings,
    meetingDetectionSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings,
    updateMeetingDetectionSettings
  } = useConfig();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);
  const [obsidianVaultPath, setObsidianVaultPath] = useState("");
  // Empty means "use the backend default". Never seed a template literal here:
  // this field used to default to the pre-{short_id} template and write it back
  // on save, silently downgrading users to collision-prone filenames.
  const [obsidianFilenameTemplate, setObsidianFilenameTemplate] = useState("");
  const [obsidianDefaultTemplate, setObsidianDefaultTemplate] = useState("");
  const [obsidianAutoExport, setObsidianAutoExport] = useState(true);
  const [isSavingObsidianPath, setIsSavingObsidianPath] = useState(false);
  const [pipelineSettings, setPipelineSettings] = useState<PipelineSettings | null>(null);
  const [isSavingPipeline, setIsSavingPipeline] = useState(false);
  const hasTrackedViewRef = useRef(false);

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
    // Reset tracking ref on mount (every tab visit)
    hasTrackedViewRef.current = false;
  }, [loadPreferences]);

  useEffect(() => {
    const loadObsidianSettings = async () => {
      if (!isTauriRuntime()) {
        return;
      }

      try {
        const settings = await invoke<ObsidianSettingsView>('get_obsidian_settings');
        setObsidianVaultPath(settings.vault_path || "");
        setObsidianFilenameTemplate(settings.filename_template ?? "");
        setObsidianDefaultTemplate(settings.default_filename_template ?? "");
        setObsidianAutoExport(settings.auto_export ?? true);
      } catch (error) {
        console.error('Failed to load Obsidian settings:', error);
      }
    };

    loadObsidianSettings();
  }, []);

  useEffect(() => {
    const loadPipelineSettings = async () => {
      if (!isTauriRuntime()) {
        return;
      }
      try {
        setPipelineSettings(await invoke<PipelineSettings>('pipeline_get_settings'));
      } catch (error) {
        console.error('Failed to load pipeline settings:', error);
      }
    };

    loadPipelineSettings();
  }, []);

  const handleSavePipelineSettings = async () => {
    if (!pipelineSettings) return;
    setIsSavingPipeline(true);
    try {
      const saved = await invoke<PipelineSettings>('pipeline_set_settings', {
        settingsUpdate: pipelineSettings,
      });
      setPipelineSettings(saved);
      toast.success('Automatic pipeline settings saved');
    } catch (error) {
      console.error('Failed to save pipeline settings:', error);
      toast.error('Failed to save automatic pipeline settings');
    } finally {
      setIsSavingPipeline(false);
    }
  };

  // Track preferences viewed analytics on every tab visit (once per mount)
  useEffect(() => {
    if (hasTrackedViewRef.current) return;

    const trackPreferencesViewed = async () => {
      // Wait for notification settings to be available (either from cache or after loading)
      if (notificationSettings) {
        await Analytics.track('preferences_viewed', {
          notifications_enabled: notificationSettings.notification_preferences.show_recording_started ? 'true' : 'false'
        });
        hasTrackedViewRef.current = true;
      } else if (!isLoadingPreferences) {
        // If not loading and no settings available, track with default value
        await Analytics.track('preferences_viewed', {
          notifications_enabled: 'false'
        });
        hasTrackedViewRef.current = true;
      }
    };

    trackPreferencesViewed();
  }, [notificationSettings, isLoadingPreferences]);

  // Update notificationsEnabled when notificationSettings are loaded from global state
  useEffect(() => {
    if (notificationSettings) {
      // Notification enabled means both started and stopped notifications are enabled
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped;
      setNotificationsEnabled(enabled);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled);
        setIsInitialLoad(false);
      }
    } else if (!isLoadingPreferences) {
      // If not loading and no settings, use default
      setNotificationsEnabled(true);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true);
        setIsInitialLoad(false);
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    // Skip update on initial load or if value hasn't actually changed
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return;
    if (!notificationSettings) return;

    const handleUpdateNotificationSettings = async () => {
      console.log("Updating notification settings to:", notificationsEnabled);

      try {
        // Update the notification preferences
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          }
        };

        console.log("Calling updateNotificationSettings with:", updatedSettings);
        await updateNotificationSettings(updatedSettings);
        setPreviousNotificationsEnabled(notificationsEnabled);
        console.log("Successfully updated notification settings to:", notificationsEnabled);

        // Track notification preference change - only fires when user manually toggles
        await Analytics.track('notification_settings_changed', {
          notifications_enabled: notificationsEnabled.toString()
        });
      } catch (error) {
        console.error('Failed to update notification settings:', error);
      }
    };

    handleUpdateNotificationSettings();
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
      }

      // Track storage folder access
      await Analytics.track('storage_folder_opened', {
        folder_type: folderType
      });
    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleSaveObsidianPath = async () => {
    if (!isTauriRuntime()) {
      toast.error('Obsidian settings are only available in the desktop app');
      return;
    }

    setIsSavingObsidianPath(true);

    try {
      const settings = await invoke<ObsidianSettingsView>('set_obsidian_settings', {
        vaultPath: obsidianVaultPath.trim() || null,
        // null, not a literal — the backend owns what an empty template means.
        filenameTemplate: obsidianFilenameTemplate.trim() || null,
        autoExport: obsidianAutoExport,
      });
      setObsidianVaultPath(settings.vault_path || "");
      setObsidianFilenameTemplate(settings.filename_template ?? "");
      setObsidianDefaultTemplate(settings.default_filename_template ?? "");
      setObsidianAutoExport(settings.auto_export ?? true);
      toast.success('Obsidian vault saved');
      await Analytics.track('obsidian_vault_path_saved', {
        configured: (!!settings.vault_path).toString(),
      });
    } catch (error) {
      console.error('Failed to save Obsidian vault path:', error);
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setIsSavingObsidianPath(false);
    }
  };

  const handleOpenObsidianMeetingsFolder = async () => {
    if (!isTauriRuntime()) {
      toast.error('Obsidian folders are only available in the desktop app');
      return;
    }

    try {
      await invoke('open_obsidian_meetings_folder');
      await Analytics.track('storage_folder_opened', {
        folder_type: 'obsidian_meetings',
      });
    } catch (error) {
      console.error('Failed to open Obsidian Meetings folder:', error);
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleClearObsidianPath = async () => {
    if (!isTauriRuntime()) {
      setObsidianVaultPath("");
      return;
    }

    setObsidianVaultPath("");

    try {
      await invoke('set_obsidian_vault_path', {
        vaultPath: null,
      });
      toast.success('Obsidian vault cleared');
    } catch (error) {
      console.error('Failed to clear Obsidian vault path:', error);
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  // Show loading if notificationsEnabled hasn't been determined yet
  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  // Ensure we have a boolean value for the Switch component
  const notificationsEnabledValue = notificationsEnabled ?? false;
  const meetingDetectionSettingsValue = meetingDetectionSettings ?? {
    meeting_detection_enabled: false,
    teams_detection_enabled: true,
    teams_prompt_start: true,
    teams_prompt_stop: true,
    teams_prompt_cooldown_minutes: 30,
  };

  const updateMeetingDetectionSetting = async (
    patch: Partial<typeof meetingDetectionSettingsValue>
  ) => {
    try {
      const updatedSettings = {
        ...meetingDetectionSettingsValue,
        ...patch,
      };
      await updateMeetingDetectionSettings(updatedSettings);
      await Analytics.track('meeting_detection_settings_changed', {
        meeting_detection_enabled: updatedSettings.meeting_detection_enabled.toString(),
        teams_detection_enabled: updatedSettings.teams_detection_enabled.toString(),
        teams_prompt_start: updatedSettings.teams_prompt_start.toString(),
        teams_prompt_stop: updatedSettings.teams_prompt_stop.toString(),
      });
    } catch (error) {
      console.error('Failed to update meeting detection settings:', error);
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="space-y-6">
      {/* Notifications Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-2">Notifications</h3>
            <p className="text-sm text-gray-600">Enable or disable notifications of start and end of meeting</p>
          </div>
          <Switch checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />
        </div>
      </div>

      {/* Meeting Detection Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="space-y-5">
          <div className="flex items-center justify-between gap-4">
            <div>
              <h3 className="text-lg font-semibold text-gray-900 mb-2">Meeting Detection</h3>
              <p className="text-sm text-gray-600">Suggest recording actions when Teams appears to be in a call</p>
            </div>
            <Switch
              checked={meetingDetectionSettingsValue.meeting_detection_enabled}
              onCheckedChange={(checked) => updateMeetingDetectionSetting({ meeting_detection_enabled: checked })}
            />
          </div>

          <div className="space-y-4 border-t border-gray-100 pt-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="font-medium text-gray-900">Detect Teams calls</div>
                <p className="text-sm text-gray-600">Use local Windows signals for the new Teams app</p>
              </div>
              <Switch
                checked={meetingDetectionSettingsValue.teams_detection_enabled}
                disabled={!meetingDetectionSettingsValue.meeting_detection_enabled}
                onCheckedChange={(checked) => updateMeetingDetectionSetting({ teams_detection_enabled: checked })}
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="font-medium text-gray-900">Suggest start recording</div>
                <p className="text-sm text-gray-600">Prompt after Teams audio is active for about 30 seconds</p>
              </div>
              <Switch
                checked={meetingDetectionSettingsValue.teams_prompt_start}
                disabled={!meetingDetectionSettingsValue.meeting_detection_enabled}
                onCheckedChange={(checked) => updateMeetingDetectionSetting({ teams_prompt_start: checked })}
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="font-medium text-gray-900">Suggest stop recording</div>
                <p className="text-sm text-gray-600">Prompt when Teams audio appears inactive for about 60 to 90 seconds</p>
              </div>
              <Switch
                checked={meetingDetectionSettingsValue.teams_prompt_stop}
                disabled={!meetingDetectionSettingsValue.meeting_detection_enabled}
                onCheckedChange={(checked) => updateMeetingDetectionSetting({ teams_prompt_stop: checked })}
              />
            </div>
          </div>
        </div>
      </div>

      {/* Outlook Calendar Section: Windows only, see docs/CALENDAR_MEETING_DETECTION_SPEC.md */}
      {platform === 'windows' && <OutlookCalendarSettings />}

      {/* Data Storage Locations Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-4">Data Storage Locations</h3>
        <p className="text-sm text-gray-600 mb-6">
          View and access where Meetily stores your data
        </p>

        <div className="space-y-4">
          {/* Database Location */}
          {/* <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Database</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.database || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('database')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div> */}

          {/* Models Location */}
          {/* <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Whisper Models</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.models || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('models')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div> */}

          {/* Recordings Location */}
          <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Meeting Recordings</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.recordings || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('recordings')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div>

          <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Obsidian Vault</div>
            <div className="flex flex-col gap-3">
              <input
                value={obsidianVaultPath}
                onChange={(event) => setObsidianVaultPath(event.target.value)}
                placeholder="D:\\Path\\To\\ObsidianVault"
                className="min-w-0 rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-mono text-gray-900"
              />
              <input
                value={obsidianFilenameTemplate}
                onChange={(event) => setObsidianFilenameTemplate(event.target.value)}
                placeholder={obsidianDefaultTemplate}
                className="min-w-0 rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-mono text-gray-900"
              />
              <div className="text-xs text-gray-600">
                Filename placeholders: {"{date}"}, {"{title}"}, {"{id}"}, {"{short_id}"}.
                {obsidianDefaultTemplate && ` Leave empty to use ${obsidianDefaultTemplate}. Including {short_id} keeps two meetings with the same date and title from overwriting each other.`}
              </div>
              <label className="flex items-center gap-2 text-sm text-gray-900 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={obsidianAutoExport}
                  onChange={(event) => setObsidianAutoExport(event.target.checked)}
                  className="h-4 w-4 rounded border-gray-300 accent-gray-900"
                />
                Auto-export summaries to Obsidian
              </label>
              <div className="text-xs text-gray-600">
                When enabled, every completed AI summary is saved to the vault automatically. Remember to Save after changing.
              </div>
              <div className="flex gap-2">
                <button
                  onClick={handleSaveObsidianPath}
                  disabled={isSavingObsidianPath}
                  className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors disabled:opacity-60"
                >
                  <Save className="w-4 h-4" />
                  Save
                </button>
                <button
                  onClick={handleClearObsidianPath}
                  className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
                >
                  <X className="w-4 h-4" />
                  Clear
                </button>
                <button
                  onClick={handleOpenObsidianMeetingsFolder}
                  className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
                >
                  <FolderOpen className="w-4 h-4" />
                  Meetings
                </button>
              </div>
            </div>
          </div>
        </div>

        <div className="mt-4 p-3 bg-blue-50 rounded-md">
          <p className="text-xs text-blue-800">
            <strong>Note:</strong> Database and models are stored together in your application data directory for unified management.
          </p>
        </div>
      </div>

      {/* Automatic pipeline */}
      {pipelineSettings && (
        <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
          <h3 className="text-lg font-semibold mb-1">Automatic Pipeline</h3>
          <p className="text-sm text-gray-600 mb-4">
            Imports new SharePoint recordings, transcribes and summarizes them, and exports the
            notes to Obsidian — running in the background while Meetily sits in the tray.
          </p>

          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="font-medium">Run automatically</div>
                <div className="text-xs text-gray-600">
                  Turn off to leave everything to the manual Process button.
                </div>
              </div>
              <Switch
                checked={pipelineSettings.enabled}
                onCheckedChange={(checked) =>
                  setPipelineSettings({ ...pipelineSettings, enabled: checked })
                }
              />
            </div>

            <div className="flex items-center justify-between">
              <div>
                <div className="font-medium">Identify speakers</div>
                <div className="text-xs text-gray-600">
                  Automatic transcriptions also work out who spoke, which takes several extra
                  minutes per meeting. On-device transcription needs the speaker model: download
                  it once from a meeting&apos;s retranscribe dialog.
                </div>
              </div>
              <Switch
                checked={pipelineSettings.diarization}
                onCheckedChange={(checked) =>
                  setPipelineSettings({ ...pipelineSettings, diarization: checked })
                }
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <label className="flex flex-col gap-1 text-sm">
                <span className="font-medium">Idle before transcribing (minutes)</span>
                <input
                  type="number"
                  min={0}
                  value={pipelineSettings.idle_minutes}
                  onChange={(event) =>
                    setPipelineSettings({
                      ...pipelineSettings,
                      idle_minutes: Number(event.target.value),
                    })
                  }
                  className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm"
                />
                <span className="text-xs text-gray-600">
                  On-device transcription waits until the machine is unused this long, and always
                  yields to a live recording.
                </span>
              </label>

              <label className="flex flex-col gap-1 text-sm">
                <span className="font-medium">SharePoint scan every (minutes)</span>
                <input
                  type="number"
                  min={1}
                  value={pipelineSettings.scan_interval_minutes}
                  onChange={(event) =>
                    setPipelineSettings({
                      ...pipelineSettings,
                      scan_interval_minutes: Number(event.target.value),
                    })
                  }
                  className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm"
                />
                <span className="text-xs text-gray-600">
                  Scans silently. If the session has expired you get a notification instead of a
                  login popup.
                </span>
              </label>

              <label className="flex flex-col gap-1 text-sm">
                <span className="font-medium">Summary template</span>
                <input
                  value={pipelineSettings.summary_template_id}
                  onChange={(event) =>
                    setPipelineSettings({
                      ...pipelineSettings,
                      summary_template_id: event.target.value,
                    })
                  }
                  className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-mono"
                />
              </label>

              <label className="flex flex-col gap-1 text-sm">
                <span className="font-medium">Retries before giving up</span>
                <input
                  type="number"
                  min={1}
                  value={pipelineSettings.max_attempts}
                  onChange={(event) =>
                    setPipelineSettings({
                      ...pipelineSettings,
                      max_attempts: Number(event.target.value),
                    })
                  }
                  className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm"
                />
                <span className="text-xs text-gray-600">
                  Only counts real failures — an unreachable summary endpoint retries indefinitely.
                </span>
              </label>
            </div>

            <div className="flex items-center gap-3">
              <button
                onClick={handleSavePipelineSettings}
                disabled={isSavingPipeline}
                className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors disabled:opacity-60"
              >
                <Save className="w-4 h-4" />
                Save
              </button>
              {pipelineSettings.last_scan_at && (
                <span className="text-xs text-gray-500">
                  Last scan: {new Date(pipelineSettings.last_scan_at).toLocaleString()}
                </span>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Analytics Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <AnalyticsConsentSwitch />
      </div>
    </div>
  )
}

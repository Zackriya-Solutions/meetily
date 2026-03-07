"use client"

import { useEffect, useState, useRef } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen, Check } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import Analytics from "@/lib/analytics"
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"
import { useTheme, THEME_OPTIONS, ThemeVariant } from "@/contexts/ThemeContext"

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings
  } = useConfig();
  const { theme, setTheme } = useTheme();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);
  const hasTrackedViewRef = useRef(false);

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
    // Reset tracking ref on mount (every tab visit)
    hasTrackedViewRef.current = false;
  }, [loadPreferences]);

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

  return (
    <div className="space-y-6">

      {/* Appearance Section */}
      <div className="bg-white dark:bg-card rounded-lg border border-gray-200 dark:border-border p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-foreground mb-1">Appearance</h3>
        <p className="text-sm text-gray-600 dark:text-muted-foreground mb-5">Choose your preferred colour theme</p>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {THEME_OPTIONS.map((opt) => {
            const active = theme === opt.value;
            const [bg, card, accent] = opt.preview;
            return (
              <button
                key={opt.value}
                onClick={() => setTheme(opt.value as ThemeVariant)}
                className={`relative rounded-xl border-2 p-3 text-left transition-all focus:outline-none ${
                  active ? 'ring-2' : 'border-gray-200 dark:border-border hover:border-gray-300 dark:hover:border-muted-foreground'
                }`}
                style={active ? {
                  borderColor: 'hsl(var(--theme-accent))',
                  '--tw-ring-color': 'hsl(var(--theme-accent) / 0.2)',
                } as React.CSSProperties : {}}
              >
                {/* Mini UI preview */}
                <div
                  className="w-full h-12 rounded-lg mb-2 overflow-hidden relative border border-black/10"
                  style={{ backgroundColor: bg }}
                >
                  {/* Simulated sidebar strip */}
                  <div className="absolute left-0 top-0 bottom-0 w-3" style={{ backgroundColor: card }} />
                  {/* Simulated card */}
                  <div
                    className="absolute left-4 top-2 right-2 h-4 rounded-sm"
                    style={{ backgroundColor: card }}
                  />
                  {/* Simulated text lines */}
                  <div
                    className="absolute left-5 top-3.5 w-6 h-1 rounded-full opacity-60"
                    style={{ backgroundColor: accent }}
                  />
                  <div
                    className="absolute left-5 top-5.5 right-3 h-1 rounded-full opacity-30"
                    style={{ backgroundColor: accent }}
                  />
                  {/* Accent dot (active indicator / button) */}
                  <div
                    className="absolute bottom-1.5 right-2 w-3 h-3 rounded-full"
                    style={{ backgroundColor: accent }}
                  />
                </div>

                <div className="text-sm font-medium text-gray-900 dark:text-foreground">{opt.label}</div>
                <div className="text-xs text-gray-500 dark:text-muted-foreground mt-0.5 leading-tight">{opt.description}</div>

                {active && (
                  <div
                    className="absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center"
                    style={{ backgroundColor: 'hsl(var(--theme-accent))' }}
                  >
                    <Check className="w-3 h-3" style={{ color: 'hsl(var(--theme-accent-fg))' }} />
                  </div>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Notifications Section */}
      <div className="bg-white dark:bg-card rounded-lg border border-gray-200 dark:border-border p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 dark:text-foreground mb-2">Notifications</h3>
            <p className="text-sm text-gray-600 dark:text-muted-foreground">Enable or disable notifications of start and end of meeting</p>
          </div>
          <Switch checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />
        </div>
      </div>

      {/* Data Storage Locations Section */}
      <div className="bg-white dark:bg-card rounded-lg border border-gray-200 dark:border-border p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-foreground mb-4">Data Storage Locations</h3>
        <p className="text-sm text-gray-600 dark:text-muted-foreground mb-6">
          View and access where Clearminutes stores your data
        </p>

        <div className="space-y-4">
          {/* Recordings Location */}
          <div className="p-4 border border-gray-200 dark:border-border rounded-lg bg-gray-50 dark:bg-secondary">
            <div className="font-medium mb-2 text-gray-900 dark:text-foreground">Meeting Recordings</div>
            <div className="text-sm text-gray-600 dark:text-muted-foreground mb-3 break-all font-mono text-xs">
              {storageLocations?.recordings || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('recordings')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 dark:border-border rounded-md hover:bg-gray-100 dark:hover:bg-accent dark:text-foreground transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div>
        </div>

        <div className="mt-4 p-3 bg-blue-50 dark:bg-secondary rounded-md">
          <p className="text-xs text-blue-800 dark:text-primary">
            <strong>Note:</strong> Database and models are stored together in your application data directory for unified management.
          </p>
        </div>
      </div>

      {/* Analytics Section */}
      <div className="bg-white dark:bg-card rounded-lg border border-gray-200 dark:border-border p-6 shadow-sm">
        <AnalyticsConsentSwitch />
      </div>
    </div>
  )
}

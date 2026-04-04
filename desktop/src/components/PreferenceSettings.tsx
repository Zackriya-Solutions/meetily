"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import Analytics from "@/lib/analytics"
import { useConfig, NotificationSettings, AppPreferences } from "@/contexts/ConfigContext"
import { Button } from "./ui/button"
import { toast } from "sonner"

interface VocabularyEntry {
  id: string
  scope_type: "global" | "meeting"
  scope_id: string | null
  source_text: string
  target_text: string
  case_sensitive: boolean
  created_at: string
  updated_at: string
}

interface MeetingOption {
  id: string
  title: string
}

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    appPreferences,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings,
    updateAppPreferences,
  } = useConfig()

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null)
  const [isInitialLoad, setIsInitialLoad] = useState(true)
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null)
  const hasTrackedViewRef = useRef(false)
  const [isUpdatingPreferences, setIsUpdatingPreferences] = useState(false)

  const [vocabularyScope, setVocabularyScope] = useState<"global" | "meeting">("global")
  const [meetingOptions, setMeetingOptions] = useState<MeetingOption[]>([])
  const [selectedMeetingScopeId, setSelectedMeetingScopeId] = useState("")
  const [vocabularyEntries, setVocabularyEntries] = useState<VocabularyEntry[]>([])
  const [isVocabularyLoading, setIsVocabularyLoading] = useState(false)
  const [isSavingVocabulary, setIsSavingVocabulary] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [sourceText, setSourceText] = useState("")
  const [targetText, setTargetText] = useState("")
  const [caseSensitive, setCaseSensitive] = useState(false)

  const defaultAppPreferences: AppPreferences = {
    auto_export_markdown_on_finalize: false,
    transcript_cleanup: {
      enabled: true,
      remove_fillers: true,
    },
  }
  const effectiveAppPreferences = appPreferences ?? defaultAppPreferences

  useEffect(() => {
    loadPreferences()
    hasTrackedViewRef.current = false
  }, [loadPreferences])

  useEffect(() => {
    if (hasTrackedViewRef.current) return

    const trackPreferencesViewed = async () => {
      if (notificationSettings) {
        await Analytics.track("preferences_viewed", {
          notifications_enabled: notificationSettings.notification_preferences.show_recording_started ? "true" : "false",
        })
        hasTrackedViewRef.current = true
      } else if (!isLoadingPreferences) {
        await Analytics.track("preferences_viewed", {
          notifications_enabled: "false",
        })
        hasTrackedViewRef.current = true
      }
    }

    void trackPreferencesViewed()
  }, [notificationSettings, isLoadingPreferences])

  useEffect(() => {
    if (notificationSettings) {
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped
      setNotificationsEnabled(enabled)
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled)
        setIsInitialLoad(false)
      }
    } else if (!isLoadingPreferences) {
      setNotificationsEnabled(true)
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true)
        setIsInitialLoad(false)
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return
    if (!notificationSettings) return

    const handleUpdateNotificationSettings = async () => {
      try {
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          },
        }

        await updateNotificationSettings(updatedSettings)
        setPreviousNotificationsEnabled(notificationsEnabled)
      } catch (error) {
        console.error("Failed to update notification settings:", error)
      }
    }

    void handleUpdateNotificationSettings()
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  useEffect(() => {
    const loadMeetingOptions = async () => {
      try {
        const rows = await invoke<MeetingOption[]>("meetings_list")
        setMeetingOptions(rows)
      } catch (error) {
        console.error("Failed to load meetings for vocabulary scopes:", error)
      }
    }

    void loadMeetingOptions()
  }, [])

  const resetVocabularyForm = useCallback(() => {
    setEditingId(null)
    setSourceText("")
    setTargetText("")
    setCaseSensitive(false)
  }, [])

  const loadVocabularyEntries = useCallback(async () => {
    if (vocabularyScope === "meeting" && !selectedMeetingScopeId) {
      setVocabularyEntries([])
      return
    }

    try {
      setIsVocabularyLoading(true)
      const rows = await invoke<VocabularyEntry[]>("vocabulary_list", {
        scopeType: vocabularyScope,
        scopeId: vocabularyScope === "meeting" ? selectedMeetingScopeId : null,
      })
      setVocabularyEntries(rows)
    } catch (error) {
      console.error("Failed to load vocabulary entries:", error)
      toast.error("Failed to load vocabulary entries")
    } finally {
      setIsVocabularyLoading(false)
    }
  }, [vocabularyScope, selectedMeetingScopeId])

  useEffect(() => {
    void loadVocabularyEntries()
  }, [loadVocabularyEntries])

  const handleOpenFolder = async (folderType: "database" | "models" | "recordings") => {
    try {
      switch (folderType) {
        case "database":
          await invoke("open_database_folder")
          break
        case "models":
          await invoke("open_models_folder")
          break
        case "recordings":
          await invoke("open_recordings_folder")
          break
      }

      await Analytics.track("storage_folder_opened", {
        folder_type: folderType,
      })
    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error)
    }
  }

  const updatePreferences = async (next: AppPreferences) => {
    try {
      setIsUpdatingPreferences(true)
      await updateAppPreferences(next)
    } catch (error) {
      console.error("Failed to update app preferences:", error)
    } finally {
      setIsUpdatingPreferences(false)
    }
  }

  const handleAutoExportToggle = (checked: boolean) => {
    void updatePreferences({
      ...effectiveAppPreferences,
      auto_export_markdown_on_finalize: checked,
    })
  }

  const handleCleanupEnabledToggle = (checked: boolean) => {
    void updatePreferences({
      ...effectiveAppPreferences,
      transcript_cleanup: {
        ...effectiveAppPreferences.transcript_cleanup,
        enabled: checked,
      },
    })
  }

  const handleRemoveFillersToggle = (checked: boolean) => {
    void updatePreferences({
      ...effectiveAppPreferences,
      transcript_cleanup: {
        ...effectiveAppPreferences.transcript_cleanup,
        remove_fillers: checked,
      },
    })
  }

  const handleSaveVocabularyEntry = async () => {
    if (!sourceText.trim() || !targetText.trim()) {
      toast.error("Source and target text are required")
      return
    }
    if (vocabularyScope === "meeting" && !selectedMeetingScopeId) {
      toast.error("Select a meeting for meeting-scoped vocabulary")
      return
    }

    try {
      setIsSavingVocabulary(true)
      await invoke("vocabulary_upsert", {
        entry: {
          id: editingId,
          scope_type: vocabularyScope,
          scope_id: vocabularyScope === "meeting" ? selectedMeetingScopeId : null,
          source_text: sourceText.trim(),
          target_text: targetText.trim(),
          case_sensitive: caseSensitive,
        },
      })
      toast.success(editingId ? "Vocabulary entry updated" : "Vocabulary entry added")
      resetVocabularyForm()
      await loadVocabularyEntries()
    } catch (error) {
      console.error("Failed to save vocabulary entry:", error)
      toast.error(`Failed to save vocabulary entry: ${String(error)}`)
    } finally {
      setIsSavingVocabulary(false)
    }
  }

  const handleEditVocabulary = (entry: VocabularyEntry) => {
    setEditingId(entry.id)
    setSourceText(entry.source_text)
    setTargetText(entry.target_text)
    setCaseSensitive(entry.case_sensitive)
  }

  const handleDeleteVocabulary = async (id: string) => {
    try {
      await invoke("vocabulary_delete", { id })
      if (editingId === id) {
        resetVocabularyForm()
      }
      await loadVocabularyEntries()
      toast.success("Vocabulary entry deleted")
    } catch (error) {
      console.error("Failed to delete vocabulary entry:", error)
      toast.error("Failed to delete vocabulary entry")
    }
  }

  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  const notificationsEnabledValue = notificationsEnabled ?? false

  return (
    <div className="space-y-6">
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-2">Notifications</h3>
            <p className="text-sm text-gray-600">Enable or disable notifications of start and end of meeting</p>
          </div>
          <Switch checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-2">Markdown Auto-Export</h3>
            <p className="text-sm text-gray-600">Automatically export markdown after recording finalization succeeds.</p>
          </div>
          <Switch
            checked={effectiveAppPreferences.auto_export_markdown_on_finalize}
            onCheckedChange={handleAutoExportToggle}
            disabled={isUpdatingPreferences}
          />
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-4">
        <h3 className="text-lg font-semibold text-gray-900">Transcript Cleanup</h3>
        <div className="flex items-center justify-between">
          <p className="text-sm text-gray-600">Enable cleanup for newly persisted transcripts.</p>
          <Switch
            checked={effectiveAppPreferences.transcript_cleanup.enabled}
            onCheckedChange={handleCleanupEnabledToggle}
            disabled={isUpdatingPreferences}
          />
        </div>
        <div className="flex items-center justify-between">
          <p className="text-sm text-gray-600">Remove filler words during cleanup.</p>
          <Switch
            checked={effectiveAppPreferences.transcript_cleanup.remove_fillers}
            onCheckedChange={handleRemoveFillersToggle}
            disabled={isUpdatingPreferences || !effectiveAppPreferences.transcript_cleanup.enabled}
          />
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-4">
        <h3 className="text-lg font-semibold text-gray-900">Vocabulary Rules</h3>
        <p className="text-sm text-gray-600">
          Rules apply to transcript display, summary generation input, and markdown export.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1">Scope</label>
            <select
              value={vocabularyScope}
              onChange={(e) => {
                const scope = e.target.value as "global" | "meeting"
                setVocabularyScope(scope)
                setEditingId(null)
                if (scope === "global") {
                  setSelectedMeetingScopeId("")
                }
              }}
              className="w-full h-9 rounded border border-gray-300 px-2 text-sm"
            >
              <option value="global">Global</option>
              <option value="meeting">Per Meeting</option>
            </select>
          </div>

          {vocabularyScope === "meeting" && (
            <div>
              <label className="block text-xs font-medium text-gray-600 mb-1">Meeting</label>
              <select
                value={selectedMeetingScopeId}
                onChange={(e) => {
                  setSelectedMeetingScopeId(e.target.value)
                  setEditingId(null)
                }}
                className="w-full h-9 rounded border border-gray-300 px-2 text-sm"
              >
                <option value="">Select meeting</option>
                {meetingOptions.map((meeting) => (
                  <option key={meeting.id} value={meeting.id}>
                    {meeting.title}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <input
            className="h-9 rounded border border-gray-300 px-3 text-sm"
            value={sourceText}
            onChange={(e) => setSourceText(e.target.value)}
            placeholder="Source text (e.g. open ai)"
          />
          <input
            className="h-9 rounded border border-gray-300 px-3 text-sm"
            value={targetText}
            onChange={(e) => setTargetText(e.target.value)}
            placeholder="Replacement text (e.g. OpenAI)"
          />
        </div>

        <div className="flex items-center justify-between">
          <label className="text-sm text-gray-700">Case sensitive match</label>
          <Switch checked={caseSensitive} onCheckedChange={setCaseSensitive} />
        </div>

        <div className="flex items-center justify-end gap-2">
          {editingId && (
            <Button variant="outline" onClick={resetVocabularyForm} disabled={isSavingVocabulary}>
              Cancel Edit
            </Button>
          )}
          <Button onClick={handleSaveVocabularyEntry} disabled={isSavingVocabulary}>
            {editingId ? "Update Rule" : "Add Rule"}
          </Button>
        </div>

        <div className="border rounded-md divide-y">
          {isVocabularyLoading ? (
            <div className="p-3 text-sm text-gray-500">Loading vocabulary rules...</div>
          ) : vocabularyEntries.length === 0 ? (
            <div className="p-3 text-sm text-gray-500">No vocabulary rules in this scope.</div>
          ) : (
            vocabularyEntries.map((entry) => (
              <div key={entry.id} className="p-3 flex items-center justify-between gap-4">
                <div className="min-w-0">
                  <p className="text-sm font-medium text-gray-900 truncate">
                    {entry.source_text} {"->"} {entry.target_text}
                  </p>
                  <p className="text-xs text-gray-500">
                    {entry.case_sensitive ? "Case sensitive" : "Case insensitive"}
                  </p>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <Button size="sm" variant="outline" onClick={() => handleEditVocabulary(entry)}>
                    Edit
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => void handleDeleteVocabulary(entry.id)}>
                    Delete
                  </Button>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-4">Data Storage Locations</h3>
        <p className="text-sm text-gray-600 mb-6">View and access where MeetFree stores your data</p>

        <div className="space-y-4">
          <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Meeting Recordings</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.recordings || "Loading..."}
            </div>
            <button
              onClick={() => void handleOpenFolder("recordings")}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div>
        </div>

        <div className="mt-4 p-3 bg-blue-50 rounded-md">
          <p className="text-xs text-blue-800">
            <strong>Note:</strong> Database and models are stored together in your application data directory for unified management.
          </p>
        </div>
      </div>
    </div>
  )
}

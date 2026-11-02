"use client"

import { useCallback, useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { toast } from "sonner"
import { CalendarClock, RefreshCw } from "lucide-react"
import { Switch } from "./ui/switch"

type CalendarAccess = "granted" | "not_determined" | "denied" | "write_only" | "unsupported"
type CallService = "google_meet" | "zoom" | "teams"
type Eligibility = "will_record" | "no_call_link" | "all_day" | "cancelled" | "not_accepted"

interface CalendarAutoRecordStatus {
  enabled: boolean
  access: CalendarAccess
}

interface UpcomingMeeting {
  title: string
  start: string
  end: string
  service: CallService | null
  eligibility: Eligibility
}

const SERVICE_NAMES: Record<CallService, string> = {
  google_meet: "Google Meet",
  zoom: "Zoom",
  teams: "Teams",
}

function describe(meeting: UpcomingMeeting): { label: string; recorded: boolean } {
  switch (meeting.eligibility) {
    case "will_record":
      return { label: `Will record (${meeting.service ? SERVICE_NAMES[meeting.service] : "call"})`, recorded: true }
    case "no_call_link":
      return { label: "No call link", recorded: false }
    case "not_accepted":
      return { label: "Not accepted", recorded: false }
    case "cancelled":
      return { label: "Cancelled", recorded: false }
    case "all_day":
      return { label: "All day", recorded: false }
  }
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" })
}

/**
 * Settings for calendar auto-record: the on/off switch, the macOS calendar
 * permission, and a preview of which upcoming meetings would be recorded, so the
 * rule can be checked before a real call depends on it.
 */
export function CalendarAutoRecordSettings() {
  const [status, setStatus] = useState<CalendarAutoRecordStatus | null>(null)
  const [upcoming, setUpcoming] = useState<UpcomingMeeting[]>([])
  const [isSaving, setIsSaving] = useState(false)
  const [isLoadingUpcoming, setIsLoadingUpcoming] = useState(false)

  const loadUpcoming = useCallback(async () => {
    setIsLoadingUpcoming(true)
    try {
      setUpcoming(await invoke<UpcomingMeeting[]>("calendar_autorecord_upcoming"))
    } catch (error) {
      console.error("Failed to load upcoming meetings:", error)
      setUpcoming([])
    } finally {
      setIsLoadingUpcoming(false)
    }
  }, [])

  useEffect(() => {
    invoke<CalendarAutoRecordStatus>("calendar_autorecord_get_status")
      .then(setStatus)
      .catch((error) => console.error("Failed to load calendar auto-record status:", error))
  }, [])

  useEffect(() => {
    if (status?.enabled && status.access === "granted") {
      loadUpcoming()
    }
  }, [status, loadUpcoming])

  const handleToggle = async (enabled: boolean) => {
    setIsSaving(true)
    try {
      const next = await invoke<CalendarAutoRecordStatus>("calendar_autorecord_set_enabled", { enabled })
      setStatus(next)
      if (enabled && !next.enabled) {
        toast.error("Calendar access is needed", {
          description: "Allow Meetily in System Settings, Privacy & Security, Calendars, then turn this on again.",
          duration: 6000,
        })
      }
    } catch (error) {
      console.error("Failed to change calendar auto-record:", error)
      toast.error("Could not change calendar auto-record", { description: String(error) })
    } finally {
      setIsSaving(false)
    }
  }

  const openCalendarPrivacy = async () => {
    try {
      await invoke("open_system_settings", { preferencePane: "Privacy_Calendars" })
    } catch (error) {
      console.error("Failed to open System Settings:", error)
    }
  }

  if (!status) {
    return null
  }

  if (status.access === "unsupported") {
    return (
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-2">Calendar auto-record</h3>
        <p className="text-sm text-gray-600">Available on macOS only.</p>
      </div>
    )
  }

  const accessBlocked = status.access === "denied" || status.access === "write_only"

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
      <div className="flex items-center justify-between gap-6">
        <div>
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Calendar auto-record</h3>
          <p className="text-sm text-gray-600">
            For meetings you have accepted that have a Google Meet, Zoom or Teams link, Meetily opens the link a
            minute before the start and records once the call begins. It stops two minutes after the call ends.
          </p>
        </div>
        <Switch checked={status.enabled} disabled={isSaving} onCheckedChange={handleToggle} />
      </div>

      <p className="mt-3 text-xs text-gray-500">
        Meetily reads the Calendar app on this Mac. To include a Google calendar, add the account in System
        Settings, Internet Accounts. Meetily must be running, and it only records calls you join on this Mac.
      </p>

      {accessBlocked && (
        <div className="mt-4 p-3 bg-amber-50 rounded-md flex items-center justify-between gap-4">
          <p className="text-xs text-amber-900">
            {status.access === "write_only"
              ? "Meetily can add to your calendar but not read it. Change it to Full Access."
              : "Meetily does not have access to your calendar."}
          </p>
          <button
            onClick={openCalendarPrivacy}
            className="shrink-0 px-3 py-2 text-sm border border-amber-300 rounded-md hover:bg-amber-100 transition-colors"
          >
            Open Settings
          </button>
        </div>
      )}

      {status.enabled && status.access === "granted" && (
        <div className="mt-4">
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2 text-sm font-medium text-gray-900">
              <CalendarClock className="w-4 h-4" />
              Next 24 hours
            </div>
            <button
              onClick={loadUpcoming}
              disabled={isLoadingUpcoming}
              className="flex items-center gap-1 px-2 py-1 text-xs border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <RefreshCw className={`w-3 h-3 ${isLoadingUpcoming ? "animate-spin" : ""}`} />
              Refresh
            </button>
          </div>
          {upcoming.length === 0 ? (
            <p className="text-xs text-gray-500">{isLoadingUpcoming ? "Loading..." : "No meetings in the next 24 hours."}</p>
          ) : (
            <ul className="divide-y divide-gray-100 border rounded-md">
              {upcoming.map((meeting) => {
                const { label, recorded } = describe(meeting)
                return (
                  <li key={`${meeting.title}-${meeting.start}`} className="flex items-center justify-between gap-4 px-3 py-2">
                    <div className="min-w-0">
                      <div className="text-sm text-gray-900 truncate">{meeting.title}</div>
                      <div className="text-xs text-gray-500">{formatTime(meeting.start)}</div>
                    </div>
                    <span className={`shrink-0 text-xs ${recorded ? "text-green-700" : "text-gray-500"}`}>{label}</span>
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      )}

      <p className="mt-4 text-xs text-gray-500">
        Recording laws differ by place, and some require everyone on a call to agree. Let attendees know when a
        call is recorded.
      </p>
    </div>
  )
}

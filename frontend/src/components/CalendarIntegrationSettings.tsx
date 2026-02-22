"use client";

import { useEffect, useMemo, useState } from "react";
import { Calendar, Link2, Unlink2 } from "lucide-react";
import { authFetch } from "@/lib/api";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";

type CalendarStatus = {
  provider: string;
  connected: boolean;
  account_email: string | null;
  connected_at: string | null;
  scopes: string[];
  can_writeback: boolean;
};

type CalendarAutomationSettings = {
  reminders_enabled: boolean;
  attendee_reminders_enabled: boolean;
  reminder_offset_minutes: number;
  recap_enabled: boolean;
  writeback_enabled: boolean;
};

const DEFAULT_SETTINGS: CalendarAutomationSettings = {
  reminders_enabled: true,
  attendee_reminders_enabled: false,
  reminder_offset_minutes: 2,
  recap_enabled: true,
  writeback_enabled: false,
};

export function CalendarIntegrationSettings() {
  const [loading, setLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isSendingTest, setIsSendingTest] = useState(false);
  const [testMeetingTitle, setTestMeetingTitle] = useState("Calendar Reminder Test");
  const [status, setStatus] = useState<CalendarStatus | null>(null);
  const [settings, setSettings] = useState<CalendarAutomationSettings>(DEFAULT_SETTINGS);

  const connectedLabel = useMemo(() => {
    if (!status?.connected) return "Not connected";
    if (status.account_email) return `Connected as ${status.account_email}`;
    return "Connected";
  }, [status]);

  const loadData = async () => {
    try {
      setLoading(true);
      const [statusRes, settingsRes] = await Promise.all([
        authFetch("/api/calendar/status"),
        authFetch("/api/calendar/settings"),
      ]);

      if (!statusRes.ok || !settingsRes.ok) {
        throw new Error("Failed to load calendar settings");
      }

      const statusData = await statusRes.json();
      const settingsData = await settingsRes.json();
      setStatus(statusData);
      setSettings(settingsData);
    } catch (error) {
      console.error("Failed to load calendar integration data:", error);
      toast.error("Failed to load calendar settings");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
    const params = new URLSearchParams(window.location.search);
    const calendarStatus = params.get("calendar");
    if (calendarStatus === "connected") {
      toast.success("Google Calendar connected");
    } else if (calendarStatus === "error") {
      const reason = params.get("reason") || "unknown_error";
      toast.error(`Calendar connect failed: ${reason}`);
    }
  }, []);

  const handleConnect = async () => {
    try {
      setIsSubmitting(true);
      const response = await authFetch("/api/calendar/google/connect", {
        method: "POST",
        body: JSON.stringify({ request_write_scope: settings.writeback_enabled }),
      });
      if (!response.ok) {
        const detail = await response.text();
        throw new Error(detail || "Failed to start OAuth");
      }
      const payload = await response.json();
      window.location.href = payload.authorization_url;
    } catch (error) {
      console.error("Calendar connect failed:", error);
      toast.error("Could not start calendar connection");
      setIsSubmitting(false);
    }
  };

  const handleDisconnect = async () => {
    try {
      setIsSubmitting(true);
      const response = await authFetch("/api/calendar/disconnect", {
        method: "POST",
        body: JSON.stringify({ provider: "google" }),
      });
      if (!response.ok) throw new Error("Failed to disconnect");
      toast.success("Calendar disconnected");
      await loadData();
    } catch (error) {
      console.error("Failed to disconnect calendar:", error);
      toast.error("Failed to disconnect calendar");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleSave = async () => {
    try {
      setIsSubmitting(true);
      const response = await authFetch("/api/calendar/settings", {
        method: "PUT",
        body: JSON.stringify(settings),
      });
      if (!response.ok) throw new Error("Failed to save settings");
      toast.success("Calendar automation settings saved");
      await loadData();
    } catch (error) {
      console.error("Failed to save settings:", error);
      toast.error("Failed to save settings");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleSendTestReminder = async () => {
    try {
      setIsSendingTest(true);
      const response = await authFetch("/api/calendar/reminders/send", {
        method: "POST",
        body: JSON.stringify({
          meeting_title: testMeetingTitle || "Calendar Reminder Test",
          meeting_start_iso: new Date(Date.now() + 2 * 60 * 1000).toISOString(),
          include_attendees: false,
        }),
      });
      if (!response.ok) {
        const detail = await response.text();
        throw new Error(detail || "Failed to send reminder");
      }
      toast.success("Reminder email sent to your host email");
    } catch (error) {
      console.error("Failed to send test reminder:", error);
      toast.error("Failed to send test reminder email");
    } finally {
      setIsSendingTest(false);
    }
  };

  if (loading) {
    return <div className="max-w-2xl mx-auto p-6">Loading Calendar settings...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-2 flex items-center gap-2">
              <Calendar className="w-5 h-5" />
              Google Calendar
            </h3>
            <p className="text-sm text-gray-600">{connectedLabel}</p>
          </div>
          {status?.connected ? (
            <button
              onClick={handleDisconnect}
              disabled={isSubmitting}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-md border border-gray-300 text-gray-700 hover:bg-gray-50 disabled:opacity-50"
            >
              <Unlink2 className="w-4 h-4" />
              Disconnect
            </button>
          ) : (
            <button
              onClick={handleConnect}
              disabled={isSubmitting}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
            >
              <Link2 className="w-4 h-4" />
              Connect
            </button>
          )}
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-5">
        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900">T-2 Reminder Email</p>
            <p className="text-sm text-gray-600">Send reminder before meeting starts</p>
          </div>
          <Switch
            checked={settings.reminders_enabled}
            onCheckedChange={(checked) =>
              setSettings((prev) => ({ ...prev, reminders_enabled: checked }))
            }
          />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900">Notify Attendees</p>
            <p className="text-sm text-gray-600">Include attendees in reminder emails</p>
          </div>
          <Switch
            checked={settings.attendee_reminders_enabled}
            onCheckedChange={(checked) =>
              setSettings((prev) => ({ ...prev, attendee_reminders_enabled: checked }))
            }
          />
        </div>

        <div>
          <label htmlFor="calendar-reminder-offset" className="block text-sm font-medium text-gray-900 mb-2">
            Reminder Offset (minutes)
          </label>
          <input
            id="calendar-reminder-offset"
            type="number"
            min={1}
            max={30}
            value={settings.reminder_offset_minutes}
            onChange={(e) =>
              setSettings((prev) => ({
                ...prev,
                reminder_offset_minutes: Number(e.target.value || 2),
              }))
            }
            className="w-28 px-3 py-2 border border-gray-300 rounded-md text-sm"
          />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900">Post-Meeting Recap Email</p>
            <p className="text-sm text-gray-600">Send notes recap to meeting attendees</p>
          </div>
          <Switch
            checked={settings.recap_enabled}
            onCheckedChange={(checked) =>
              setSettings((prev) => ({ ...prev, recap_enabled: checked }))
            }
          />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900">Calendar Writeback</p>
            <p className="text-sm text-gray-600">Append summary to event description (optional)</p>
          </div>
          <Switch
            checked={settings.writeback_enabled}
            onCheckedChange={(checked) =>
              setSettings((prev) => ({ ...prev, writeback_enabled: checked }))
            }
          />
        </div>

        <div className="pt-2">
          <button
            onClick={handleSave}
            disabled={isSubmitting}
            className="px-4 py-2 rounded-md bg-gray-900 text-white hover:bg-black disabled:opacity-50"
          >
            Save Calendar Settings
          </button>
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-4">
        <div>
          <p className="font-medium text-gray-900">Reminder Email Test</p>
          <p className="text-sm text-gray-600">
            Sends a reminder email with a Start Meeting button to your host email.
          </p>
        </div>
        <div>
          <label htmlFor="calendar-test-title" className="block text-sm font-medium text-gray-900 mb-2">
            Meeting Title
          </label>
          <input
            id="calendar-test-title"
            type="text"
            value={testMeetingTitle}
            onChange={(e) => setTestMeetingTitle(e.target.value)}
            className="w-full max-w-md px-3 py-2 border border-gray-300 rounded-md text-sm"
          />
        </div>
        <div>
          <button
            onClick={handleSendTestReminder}
            disabled={isSendingTest}
            className="px-4 py-2 rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {isSendingTest ? "Sending..." : "Send Test Reminder Email"}
          </button>
        </div>
      </div>
    </div>
  );
}

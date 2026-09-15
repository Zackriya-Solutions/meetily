'use client';

import { useEffect, useState } from 'react';
import { CalendarClock, CheckCircle2, Loader2, ShieldCheck } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import {
  CalendarAutomationSettings as CalendarSettings,
  DEFAULT_CALENDAR_AUTOMATION_SETTINGS,
  filterCalendarMeetings,
  isGoogleCalendarIcsUrl,
  parseGoogleCalendarIcs,
  selectNextCalendarMeeting,
} from '@/lib/calendar-automation';
import {
  fetchGoogleCalendarIcs,
  loadCalendarAutomationSettings,
  saveCalendarAutomationSettings,
} from '@/services/calendarAutomationService';

export function CalendarAutomationSettings() {
  const [settings, setSettings] = useState<CalendarSettings>(
    DEFAULT_CALENDAR_AUTOMATION_SETTINGS,
  );
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [connectionMessage, setConnectionMessage] = useState<string | null>(null);

  useEffect(() => {
    loadCalendarAutomationSettings()
      .then(setSettings)
      .catch((error) => {
        console.error('[CalendarAutomationSettings] Failed to load settings:', error);
        toast.error('Could not load calendar automation settings');
      })
      .finally(() => setLoading(false));
  }, []);

  const testConnection = async () => {
    if (!isGoogleCalendarIcsUrl(settings.icalUrl)) {
      toast.error('Enter your Google Calendar Secret address in iCal format');
      return;
    }

    setTesting(true);
    setConnectionMessage(null);
    try {
      const ics = await fetchGoogleCalendarIcs(settings.icalUrl);
      const now = new Date();
      const rangeEnd = new Date(now.getTime() + 30 * 24 * 60 * 60 * 1000);
      const meetings = filterCalendarMeetings(
        parseGoogleCalendarIcs(ics, new Date(now.getTime() - 24 * 60 * 60 * 1000), rangeEnd),
        settings.conferenceOnly,
      );
      const next = selectNextCalendarMeeting(meetings, now);
      const message = next
        ? `Connected. Next meeting: ${next.title} — ${next.start.toLocaleString()}`
        : 'Connected. No matching meetings were found in the next 30 days.';
      setConnectionMessage(message);
      toast.success('Google Calendar connected');
    } catch (error) {
      console.error('[CalendarAutomationSettings] Calendar test failed:', error);
      toast.error('Google Calendar connection failed', {
        description: String(error),
      });
    } finally {
      setTesting(false);
    }
  };

  const saveSettings = async () => {
    if (settings.enabled && !isGoogleCalendarIcsUrl(settings.icalUrl)) {
      toast.error('A valid Google Calendar iCal address is required before enabling automation');
      return;
    }

    setSaving(true);
    try {
      const saved = await saveCalendarAutomationSettings(settings);
      setSettings(saved);
      toast.success(saved.enabled ? 'Calendar auto-recording enabled' : 'Calendar auto-recording disabled');
    } catch (error) {
      console.error('[CalendarAutomationSettings] Failed to save:', error);
      toast.error('Could not save calendar settings', { description: String(error) });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return <div className="p-6 text-sm text-gray-600">Loading calendar settings...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-start justify-between gap-6">
          <div className="flex gap-3">
            <CalendarClock className="h-6 w-6 text-blue-600 mt-0.5" />
            <div>
              <h3 className="text-lg font-semibold text-gray-900">Google Calendar auto-recording</h3>
              <p className="text-sm text-gray-600 mt-1">
                Start recording when a scheduled online meeting begins and optionally stop at its calendar end time.
              </p>
            </div>
          </div>
          <Switch
            checked={settings.enabled}
            onCheckedChange={(enabled) => setSettings((current) => ({ ...current, enabled }))}
          />
        </div>
      </div>

      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-5">
        <div>
          <Label htmlFor="google-calendar-ical-url">Secret address in iCal format</Label>
          <Input
            id="google-calendar-ical-url"
            type="password"
            value={settings.icalUrl}
            onChange={(event) => {
              setConnectionMessage(null);
              setSettings((current) => ({ ...current, icalUrl: event.target.value }));
            }}
            placeholder="https://calendar.google.com/calendar/ical/.../basic.ics"
            className="mt-2"
            autoComplete="off"
          />
          <p className="text-xs text-gray-500 mt-2">
            In Google Calendar, open Settings → your calendar → Integrate calendar, then copy
            “Secret address in iCal format.” This address is stored only on this computer.
          </p>
        </div>

        <div className="flex items-center justify-between gap-6 p-4 border rounded-lg">
          <div>
            <div className="font-medium text-gray-900">Online meetings only</div>
            <div className="text-sm text-gray-600 mt-1">
              Ignore focus blocks and appointments unless they contain a Meet, Zoom, Teams, Webex,
              Slack, Discord, or Whereby link.
            </div>
          </div>
          <Switch
            checked={settings.conferenceOnly}
            onCheckedChange={(conferenceOnly) =>
              setSettings((current) => ({ ...current, conferenceOnly }))
            }
          />
        </div>

        <div className="flex items-center justify-between gap-6 p-4 border rounded-lg">
          <div>
            <div className="font-medium text-gray-900">Stop at scheduled end</div>
            <div className="text-sm text-gray-600 mt-1">
              Only recordings started by calendar automation are stopped automatically.
            </div>
          </div>
          <Switch
            checked={settings.autoStop}
            onCheckedChange={(autoStop) => setSettings((current) => ({ ...current, autoStop }))}
          />
        </div>

        {connectionMessage && (
          <div className="flex items-start gap-2 p-3 rounded-md bg-green-50 text-sm text-green-800">
            <CheckCircle2 className="h-5 w-5 flex-shrink-0" />
            <span>{connectionMessage}</span>
          </div>
        )}

        <div className="flex gap-3 justify-end">
          <Button variant="outline" onClick={testConnection} disabled={testing || saving}>
            {testing && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
            Test calendar
          </Button>
          <Button onClick={saveSettings} disabled={saving || testing}>
            {saving && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
            Save
          </Button>
        </div>
      </div>

      <div className="flex items-start gap-3 p-4 bg-blue-50 border border-blue-200 rounded-lg">
        <ShieldCheck className="h-5 w-5 text-blue-700 flex-shrink-0 mt-0.5" />
        <p className="text-sm text-blue-900">
          Meetily can run hidden in the system tray. A scheduled recording brings the window
          forward and returns it to the tray afterward if it was previously hidden. Calendar
          events are read-only; recording still happens locally. Inform participants and follow
          your organization’s recording policy.
        </p>
      </div>
    </div>
  );
}

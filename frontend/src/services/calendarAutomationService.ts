import { invoke } from '@tauri-apps/api/core';
import { Store } from '@tauri-apps/plugin-store';
import {
  CalendarAutomationSettings,
  DEFAULT_CALENDAR_AUTOMATION_SETTINGS,
} from '@/lib/calendar-automation';

const STORE_PATH = 'calendar-automation.json';
const SETTINGS_KEY = 'settings';

export const CALENDAR_SETTINGS_CHANGED_EVENT = 'calendar-automation-settings-changed';
export const CALENDAR_RECORDING_STARTED_EVENT = 'calendar-recording-started';

function normalizeSettings(
  settings: Partial<CalendarAutomationSettings> | null,
): CalendarAutomationSettings {
  return {
    ...DEFAULT_CALENDAR_AUTOMATION_SETTINGS,
    ...settings,
    icalUrl: settings?.icalUrl?.trim() ?? '',
  };
}

export async function loadCalendarAutomationSettings(): Promise<CalendarAutomationSettings> {
  const store = await Store.load(STORE_PATH);
  const settings = await store.get<Partial<CalendarAutomationSettings>>(SETTINGS_KEY);
  return normalizeSettings(settings ?? null);
}

export async function saveCalendarAutomationSettings(
  settings: CalendarAutomationSettings,
): Promise<CalendarAutomationSettings> {
  const normalized = normalizeSettings(settings);
  const store = await Store.load(STORE_PATH);
  await store.set(SETTINGS_KEY, normalized);
  await store.save();
  window.dispatchEvent(new CustomEvent(CALENDAR_SETTINGS_CHANGED_EVENT));
  return normalized;
}

export async function fetchGoogleCalendarIcs(icalUrl: string): Promise<string> {
  return invoke<string>('fetch_google_calendar_ics', { icalUrl });
}

'use client';

import { useCallback, useEffect, useRef } from 'react';
import { appDataDir } from '@tauri-apps/api/path';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { usePathname, useRouter } from 'next/navigation';
import { toast } from 'sonner';
import {
  CalendarAutomationSettings,
  CalendarMeeting,
  filterCalendarMeetings,
  parseGoogleCalendarIcs,
  selectActiveCalendarMeeting,
} from '@/lib/calendar-automation';
import {
  CALENDAR_RECORDING_STARTED_EVENT,
  CALENDAR_SETTINGS_CHANGED_EVENT,
  fetchGoogleCalendarIcs,
  loadCalendarAutomationSettings,
} from '@/services/calendarAutomationService';
import { recordingService } from '@/services/recordingService';
import { useRecordingState } from '@/contexts/RecordingStateContext';

const CALENDAR_REFRESH_MS = 5 * 60 * 1000;
const START_RETRY_MS = 2 * 60 * 1000;
const START_CONFIRMATION_TIMEOUT_MS = 45 * 1000;
const LOOK_BEHIND_MS = 24 * 60 * 60 * 1000;
const LOOK_AHEAD_MS = 30 * 24 * 60 * 60 * 1000;

interface PendingStart {
  meeting: CalendarMeeting;
  requestedAt: number;
}

export function CalendarAutomationProvider({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const recordingState = useRecordingState();

  const settingsRef = useRef<CalendarAutomationSettings | null>(null);
  const meetingsRef = useRef<CalendarMeeting[]>([]);
  const isRecordingRef = useRef(recordingState.isRecording);
  const pathnameRef = useRef(pathname);
  const pendingStartRef = useRef<PendingStart | null>(null);
  const calendarStartedMeetingRef = useRef<CalendarMeeting | null>(null);
  const lastStartAttemptRef = useRef(new Map<string, number>());
  const lastRefreshRef = useRef(0);
  const refreshInFlightRef = useRef(false);
  const stopInFlightRef = useRef(false);
  const windowWasVisibleBeforeStartRef = useRef<boolean | null>(null);
  const lastErrorRef = useRef<string | null>(null);

  useEffect(() => {
    isRecordingRef.current = recordingState.isRecording;

    if (!recordingState.isRecording && calendarStartedMeetingRef.current && !stopInFlightRef.current) {
      // A manual stop must never be restarted or stopped again by automation.
      calendarStartedMeetingRef.current = null;
    }
  }, [recordingState.isRecording]);

  useEffect(() => {
    pathnameRef.current = pathname;
  }, [pathname]);

  const refreshCalendar = useCallback(async (force = false) => {
    const settings = settingsRef.current;
    const now = Date.now();
    if (
      !settings?.enabled
      || !settings.icalUrl
      || refreshInFlightRef.current
      || (!force && now - lastRefreshRef.current < CALENDAR_REFRESH_MS)
    ) {
      return;
    }

    refreshInFlightRef.current = true;
    try {
      const ics = await fetchGoogleCalendarIcs(settings.icalUrl);
      const rangeStart = new Date(now - LOOK_BEHIND_MS);
      const rangeEnd = new Date(now + LOOK_AHEAD_MS);
      meetingsRef.current = filterCalendarMeetings(
        parseGoogleCalendarIcs(ics, rangeStart, rangeEnd),
        settings.conferenceOnly,
      );
      lastRefreshRef.current = now;
      lastErrorRef.current = null;
    } catch (error) {
      const message = String(error);
      console.error('[CalendarAutomation] Calendar refresh failed:', message);
      if (lastErrorRef.current !== message) {
        toast.error('Google Calendar could not be refreshed', {
          description: message,
          duration: 8000,
        });
        lastErrorRef.current = message;
      }
    } finally {
      refreshInFlightRef.current = false;
    }
  }, []);

  const requestCalendarStart = useCallback(async (meeting: CalendarMeeting) => {
    const now = Date.now();
    pendingStartRef.current = { meeting, requestedAt: now };
    lastStartAttemptRef.current.set(meeting.id, now);

    try {
      const appWindow = getCurrentWindow();
      windowWasVisibleBeforeStartRef.current = await appWindow.isVisible();
      await appWindow.unminimize();
      await appWindow.show();
      await appWindow.setFocus();
    } catch (error) {
      // Window visibility should never prevent a scheduled recording.
      console.error('[CalendarAutomation] Could not show the meeting window:', error);
    }

    toast.info('Scheduled meeting detected', {
      description: `Starting “${meeting.title}” automatically.`,
    });

    if (pathnameRef.current === '/') {
      window.dispatchEvent(new CustomEvent('start-recording-from-sidebar', {
        detail: { meetingTitle: meeting.title, calendarMeetingId: meeting.id },
      }));
      return;
    }

    sessionStorage.setItem('autoStartRecording', 'true');
    sessionStorage.setItem('autoStartMeetingTitle', meeting.title);
    sessionStorage.setItem('autoStartCalendarMeetingId', meeting.id);
    router.push('/');
  }, [router]);

  useEffect(() => {
    const confirmCalendarStart = (event: Event) => {
      const meetingId = event instanceof CustomEvent
        ? String(event.detail?.calendarMeetingId ?? '')
        : '';
      const pending = pendingStartRef.current;
      if (!pending || pending.meeting.id !== meetingId) return;

      calendarStartedMeetingRef.current = pending.meeting;
      pendingStartRef.current = null;
      toast.success('Calendar recording started', {
        description: calendarStartedMeetingRef.current.title,
      });
    };

    window.addEventListener(CALENDAR_RECORDING_STARTED_EVENT, confirmCalendarStart);
    return () => window.removeEventListener(CALENDAR_RECORDING_STARTED_EVENT, confirmCalendarStart);
  }, []);

  const stopCalendarRecording = useCallback(async (meeting: CalendarMeeting) => {
    if (stopInFlightRef.current) return;
    stopInFlightRef.current = true;
    try {
      const dataDir = await appDataDir();
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const savePath = `${dataDir}/recording-${timestamp}.wav`;
      await recordingService.stopRecording(savePath);
      await emit('recording-stop-complete', true);
      calendarStartedMeetingRef.current = null;
      toast.success('Calendar recording stopped', {
        description: `${meeting.title} reached its scheduled end time.`,
      });

      const wasVisible = windowWasVisibleBeforeStartRef.current;
      windowWasVisibleBeforeStartRef.current = null;
      if (wasVisible === false) {
        try {
          await getCurrentWindow().hide();
        } catch (error) {
          console.error('[CalendarAutomation] Could not return Meetily to the tray:', error);
        }
      }
    } catch (error) {
      console.error('[CalendarAutomation] Automatic stop failed:', error);
      toast.error('Could not stop the calendar recording automatically', {
        description: String(error),
        duration: 8000,
      });
    } finally {
      stopInFlightRef.current = false;
    }
  }, []);

  const evaluateAutomation = useCallback(async () => {
    const settings = settingsRef.current;
    if (!settings?.enabled) return;

    const now = new Date();
    const nowMs = now.getTime();
    const calendarStarted = calendarStartedMeetingRef.current;

    if (
      calendarStarted
      && settings.autoStop
      && isRecordingRef.current
      && nowMs >= calendarStarted.end.getTime()
    ) {
      await stopCalendarRecording(calendarStarted);
      return;
    }

    const pending = pendingStartRef.current;
    if (pending && nowMs - pending.requestedAt >= START_CONFIRMATION_TIMEOUT_MS) {
      pendingStartRef.current = null;
    }

    const activeMeeting = selectActiveCalendarMeeting(meetingsRef.current, now);
    if (!activeMeeting || isRecordingRef.current || pendingStartRef.current) {
      return;
    }

    const lastAttempt = lastStartAttemptRef.current.get(activeMeeting.id) ?? 0;
    if (nowMs - lastAttempt < START_RETRY_MS) {
      return;
    }

    await requestCalendarStart(activeMeeting);
  }, [requestCalendarStart, stopCalendarRecording]);

  const loadSettingsAndRefresh = useCallback(async () => {
    try {
      const settings = await loadCalendarAutomationSettings();
      settingsRef.current = settings;
      lastRefreshRef.current = 0;
      lastErrorRef.current = null;

      if (!settings.enabled) {
        meetingsRef.current = [];
        pendingStartRef.current = null;
        calendarStartedMeetingRef.current = null;
        windowWasVisibleBeforeStartRef.current = null;
        return;
      }

      await refreshCalendar(true);
      await evaluateAutomation();
    } catch (error) {
      console.error('[CalendarAutomation] Failed to load settings:', error);
      toast.error('Calendar automation could not start', { description: String(error) });
    }
  }, [evaluateAutomation, refreshCalendar]);

  useEffect(() => {
    void loadSettingsAndRefresh();
    window.addEventListener(CALENDAR_SETTINGS_CHANGED_EVENT, loadSettingsAndRefresh);
    return () => window.removeEventListener(CALENDAR_SETTINGS_CHANGED_EVENT, loadSettingsAndRefresh);
  }, [loadSettingsAndRefresh]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen('calendar-automation-tick', () => {
      void refreshCalendar();
      void evaluateAutomation();
    }).then((listener) => {
      if (cancelled) {
        listener();
      } else {
        unlisten = listener;
      }
    }).catch((error) => {
      console.error('[CalendarAutomation] Failed to register native tick listener:', error);
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [evaluateAutomation, refreshCalendar]);

  return <>{children}</>;
}

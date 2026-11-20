'use client';

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { format, isToday, isTomorrow, parseISO } from 'date-fns';
import {
  BookOpen,
  CalendarDays,
  CheckCircle2,
  Circle,
  FileText,
  GlobeIcon,
  Mic,
  Settings,
  Sparkles,
  Video,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSidebar, type CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { MISSING_STAGE_FILTERS, isMissingStage, type MeetingStage } from '@/components/Sidebar/meetingStages';
import { useConfig } from '@/contexts/ConfigContext';
import { useTauriEvent } from '@/hooks/useTauriEvent';
import { ModalType } from '@/hooks/useModalState';
import { BackgroundJobsPanel } from '@/components/shared/BackgroundJobsPanel';
import { PendingWorkPanel } from '@/components/shared/PendingWorkPanel';
import { TeamsRecapSync } from '@/components/TeamsRecapSync';
import { armJoinAndRecord, stashCalendarEventForAutoStart } from '@/lib/calendar-handoff';
import Analytics from '@/lib/analytics';
import { cn } from '@/lib/utils';
import type { CalendarEvent } from '@/types/calendar';

const isTauriRuntime = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Mirrors `ObsidianSettingsView` (src-tauri/src/obsidian.rs); only the vault is read here. */
interface ObsidianSettingsView {
  vault_path: string | null;
}

/** Mirrors `CalendarStatus` (src-tauri/src/calendar/types.rs); only `running` is read here. */
interface CalendarStatus {
  running: boolean;
}

interface HomeDashboardProps {
  /** The start button, mounted here while idle so its listeners stay alive. */
  recordingControls: ReactNode;
  showModal: (name: ModalType, message?: string) => void;
}

/** A recording that started inside the event (10 min early counts), or carries its title. */
function recordingFor(event: CalendarEvent, meetings: CurrentMeeting[]): CurrentMeeting | undefined {
  const start = parseISO(event.start).getTime() - 10 * 60 * 1000;
  const end = parseISO(event.end).getTime();
  return meetings.find((meeting) => {
    if (meeting.title === event.title) return true;
    if (!meeting.createdAt) return false;
    const at = parseISO(meeting.createdAt).getTime();
    return at >= start && at <= end;
  });
}

const STAGE_ICON: Record<MeetingStage, typeof FileText> = {
  transcribed: FileText,
  summarized: Sparkles,
  obsidianExported: BookOpen,
};

const STAGE_TITLE: Record<MeetingStage, string> = {
  transcribed: 'Transcript',
  summarized: 'Summary',
  obsidianExported: 'Obsidian',
};

function StageDots({ meeting }: { meeting: CurrentMeeting }) {
  return (
    <span className="flex items-center gap-1" aria-label="Pipeline stages">
      {MISSING_STAGE_FILTERS.map(({ stage }) => {
        const done = Boolean(meeting[stage]);
        const Icon = done ? CheckCircle2 : Circle;
        return (
          <Icon
            key={stage}
            className={cn('w-3.5 h-3.5', done ? 'text-emerald-500' : 'text-gray-300')}
            aria-label={`${STAGE_TITLE[stage]} ${done ? 'done' : 'missing'}`}
          />
        );
      })}
    </span>
  );
}

function Section({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section>
      <div className="flex items-center justify-between mb-2">
        <h2 className="text-xs font-medium text-gray-500 uppercase tracking-wide">{title}</h2>
        {action}
      </div>
      {children}
    </section>
  );
}

function Chip({ label, value, muted, onClick }: { label: string; value: string; muted?: boolean; onClick?: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex items-center gap-1.5 px-2.5 py-1 rounded-full border text-xs transition-colors',
        muted ? 'border-dashed border-gray-300 text-gray-400' : 'border-gray-200 bg-white text-gray-700',
        onClick && 'hover:bg-gray-100'
      )}
    >
      <span className="text-gray-400">{label}</span>
      <span className="font-medium truncate max-w-[180px]">{value}</span>
    </button>
  );
}

/**
 * The idle home screen: today's calendar, what still needs work, the latest
 * meetings, and the pipeline queue. Replaces the "Welcome to meetily" empty
 * state; the live transcript takes the space back once a recording starts.
 */
export function HomeDashboard({ recordingControls, showModal }: HomeDashboardProps) {
  const router = useRouter();
  const { meetings, missingFilter, setMissingFilter, isCollapsed, toggleCollapse } = useSidebar();
  const {
    transcriptModelConfig,
    modelConfig,
    selectedDevices,
    calendarSettings,
    meetingDetectionSettings,
    loadPreferences,
  } = useConfig();

  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [calendarRunning, setCalendarRunning] = useState<boolean | null>(null);
  const [vaultPath, setVaultPath] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    void loadPreferences();
  }, [loadPreferences]);

  // Minute clock for "ended / in progress / starts in" labels.
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);

  const refreshCalendar = useCallback(async () => {
    if (!isTauriRuntime()) return;
    try {
      const [list, status] = await Promise.all([
        invoke<CalendarEvent[]>('get_calendar_events'),
        invoke<CalendarStatus>('get_calendar_status'),
      ]);
      setEvents(list);
      setCalendarRunning(status.running);
    } catch (error) {
      console.warn('[HomeDashboard] Calendar unavailable:', error);
      setCalendarRunning(false);
    }
  }, []);

  useEffect(() => {
    void refreshCalendar();
  }, [refreshCalendar]);
  // The manager emits this after every poll and settings change.
  useTauriEvent('calendar-status-changed', () => { void refreshCalendar(); });

  useEffect(() => {
    if (!isTauriRuntime()) return;
    invoke<ObsidianSettingsView>('get_obsidian_settings')
      .then((settings) => setVaultPath(settings.vault_path))
      .catch((error) => console.warn('[HomeDashboard] Obsidian settings unavailable:', error));
  }, []);

  const todaysEvents = useMemo(
    () => events.filter((event) => isToday(parseISO(event.start))),
    [events]
  );
  const nextTomorrow = useMemo(
    () => events.find((event) => isTomorrow(parseISO(event.start))),
    [events]
  );

  const missingCounts = useMemo(
    () => Object.fromEntries(
      MISSING_STAGE_FILTERS.map(({ stage }) => [stage, meetings.filter((m) => isMissingStage(m, stage)).length])
    ) as Record<MeetingStage, number>,
    [meetings]
  );

  const recent = useMemo(() => meetings.slice(0, 6), [meetings]);

  const canJoinAndRecord = Boolean(
    meetingDetectionSettings?.meeting_detection_enabled && meetingDetectionSettings?.teams_detection_enabled
  );

  const recordEvent = useCallback((event: CalendarEvent) => {
    Analytics.trackButtonClick('home_calendar_record', 'home_dashboard');
    // Same hand-off as the nudge toast: the auto-start path on reload picks
    // up the event and its title.
    stashCalendarEventForAutoStart(event);
    window.location.assign('/');
  }, []);

  const joinAndRecord = useCallback(async (event: CalendarEvent) => {
    Analytics.trackButtonClick('home_calendar_join_and_record', 'home_dashboard');
    try {
      await invoke('open_teams_join_url', { url: event.teams_join_url });
    } catch (error) {
      toast.error('Could not open the Teams link', {
        description: error instanceof Error ? error.message : String(error),
      });
      return;
    }
    armJoinAndRecord(event);
    toast.success('Recording will start when the Teams call begins', {
      description: `Meetily is waiting for "${event.title}". The wait ends after 10 minutes.`,
      duration: 8000,
    });
  }, []);

  const showMissing = useCallback((stage: MeetingStage) => {
    setMissingFilter(missingFilter === stage ? null : stage);
    if (isCollapsed) toggleCollapse();
  }, [isCollapsed, missingFilter, setMissingFilter, toggleCollapse]);

  const openMeeting = useCallback((id: string) => {
    router.push(`/meeting-details?id=${id}`);
  }, [router]);

  const summaryModel = modelConfig.provider === 'custom-openai'
    ? (modelConfig.customOpenAIModel ?? modelConfig.model)
    : modelConfig.model;

  return (
    <div className="flex-1 min-h-0 overflow-y-auto">
      <div className="max-w-5xl mx-auto px-6 py-6 space-y-6">
        {/* Header: date and status strip */}
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h1 className="text-xl font-semibold text-gray-900">{format(now, 'EEEE, d MMMM')}</h1>
            <p className="text-sm text-gray-500">
              {todaysEvents.length === 0
                ? 'Nothing on the calendar today'
                : `${todaysEvents.length} ${todaysEvents.length === 1 ? 'meeting' : 'meetings'} today`}
            </p>
          </div>
          <div className="flex flex-wrap gap-1.5">
            <Chip label="Transcribe" value={`${transcriptModelConfig.provider} · ${transcriptModelConfig.model}`} onClick={() => router.push('/settings')} />
            <Chip label="Summarize" value={`${modelConfig.provider} · ${summaryModel}`} onClick={() => router.push('/settings')} />
            <Chip
              label="Obsidian"
              value={vaultPath ? vaultPath.split(/[\\/]/).filter(Boolean).pop() ?? vaultPath : 'no vault'}
              muted={!vaultPath}
              onClick={() => router.push('/settings')}
            />
            <Chip
              label="Outlook"
              value={calendarRunning ? 'connected' : calendarSettings?.calendar_enabled ? 'starting' : 'off'}
              muted={!calendarRunning}
              onClick={() => router.push('/settings')}
            />
            {transcriptModelConfig.provider === 'localWhisper' && (
              <Button variant="outline" size="sm" className="h-7 rounded-full" onClick={() => showModal('languageSettings')} title="Language">
                <GlobeIcon />
                Language
              </Button>
            )}
          </div>
        </div>

        {/* Hero: record */}
        <div className="bg-white rounded-xl border border-gray-200 shadow-sm p-5 flex items-center gap-5">
          <div className="flex-shrink-0">{recordingControls}</div>
          <div className="min-w-0 flex-1">
            <p className="text-base font-medium text-gray-900">Record a meeting</p>
            <p className="text-sm text-gray-500 truncate">
              <Mic className="inline w-3.5 h-3.5 mr-1 -mt-0.5" />
              {selectedDevices?.micDevice || 'Default microphone'}
              <span className="mx-2 text-gray-300">·</span>
              {selectedDevices?.systemDevice || 'Default system audio'}
            </p>
          </div>
          <Button variant="ghost" size="sm" onClick={() => showModal('deviceSettings')} title="Choose devices">
            <Settings />
            Devices
          </Button>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Today */}
          <div className="lg:col-span-2">
            <Section
              title="Today"
              action={calendarRunning === false && (
                <button type="button" className="text-xs text-blue-600 hover:underline" onClick={() => router.push('/settings')}>
                  Connect Outlook
                </button>
              )}
            >
              <div className="bg-white rounded-xl border border-gray-200 shadow-sm divide-y divide-gray-100">
                {todaysEvents.length === 0 ? (
                  <p className="p-4 text-sm text-gray-400">
                    {calendarRunning
                      ? nextTomorrow
                        ? `Next up tomorrow: ${nextTomorrow.title} at ${format(parseISO(nextTomorrow.start), 'HH:mm')}`
                        : 'No meetings today or tomorrow'
                      : 'Connect Outlook to see today\'s meetings here'}
                  </p>
                ) : (
                  todaysEvents.map((event) => {
                    const start = parseISO(event.start);
                    const end = parseISO(event.end);
                    const ended = end.getTime() <= now;
                    const live = !ended && start.getTime() <= now;
                    const recording = ended || live ? recordingFor(event, meetings) : undefined;
                    const minutesUntil = Math.ceil((start.getTime() - now) / 60_000);
                    return (
                      <div key={event.id} className={cn('flex items-center gap-4 px-4 py-3', ended && 'opacity-70')}>
                        <div className="w-24 flex-shrink-0 text-sm tabular-nums text-gray-600">
                          {format(start, 'HH:mm')}<span className="text-gray-300"> – </span>{format(end, 'HH:mm')}
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className="text-sm font-medium text-gray-900 truncate">
                            {event.teams_join_url && <Video className="inline w-3.5 h-3.5 mr-1 -mt-0.5 text-violet-500" aria-label="Teams meeting" />}
                            {event.title}
                          </p>
                          <p className="text-xs text-gray-500 truncate">
                            {event.organizer ?? 'No organizer'}
                            {event.attendees.length > 0 && ` · ${event.attendees.length} attendees`}
                            {live && <span className="ml-2 text-red-600 font-medium">In progress</span>}
                            {!ended && !live && minutesUntil <= 60 && (
                              <span className="ml-2 text-amber-600">starts in {minutesUntil} min</span>
                            )}
                          </p>
                        </div>
                        <div className="flex-shrink-0 flex items-center gap-2">
                          {recording ? (
                            <button
                              type="button"
                              onClick={() => openMeeting(recording.id)}
                              className="flex items-center gap-2 text-xs text-gray-600 hover:text-gray-900"
                            >
                              <StageDots meeting={recording} />
                              Recorded
                            </button>
                          ) : ended ? (
                            <span className="text-xs text-gray-400">Not recorded</span>
                          ) : (
                            <>
                              {canJoinAndRecord && event.teams_join_url && (
                                <Button size="sm" variant="outline" onClick={() => void joinAndRecord(event)}>
                                  Join and record
                                </Button>
                              )}
                              <Button size="sm" onClick={() => recordEvent(event)}>
                                <Mic />
                                Record
                              </Button>
                            </>
                          )}
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </Section>
          </div>

          {/* Needs attention */}
          <Section title="Needs attention">
            <div className="grid grid-cols-3 lg:grid-cols-1 gap-3">
              {MISSING_STAGE_FILTERS.map(({ stage, label }) => {
                const Icon = STAGE_ICON[stage];
                const count = missingCounts[stage];
                const active = missingFilter === stage;
                return (
                  <button
                    key={stage}
                    type="button"
                    aria-pressed={active}
                    onClick={() => showMissing(stage)}
                    className={cn(
                      'flex items-center gap-3 p-3 rounded-xl border text-left transition-colors',
                      active ? 'border-blue-500 bg-blue-50' : 'border-gray-200 bg-white hover:bg-gray-50',
                      count === 0 && !active && 'opacity-60'
                    )}
                  >
                    <Icon className={cn('w-5 h-5 flex-shrink-0', count ? 'text-amber-500' : 'text-emerald-500')} />
                    <span className="min-w-0">
                      <span className="block text-lg font-semibold leading-tight tabular-nums">{count}</span>
                      <span className="block text-xs text-gray-500 truncate">{label}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </Section>
        </div>

        {/* Sync needs a saved vault: one typed into Settings but not saved does not count. */}
        <Section title="Teams recaps">
          <TeamsRecapSync vaultConfigured={Boolean(vaultPath)} />
        </Section>

        {/* Recent meetings */}
        <Section
          title="Recent meetings"
          action={<span className="text-xs text-gray-400">{meetings.length} total</span>}
        >
          {recent.length === 0 ? (
            <p className="text-sm text-gray-400">No meetings yet. Record one or import audio from the sidebar.</p>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
              {recent.map((meeting) => (
                <button
                  key={meeting.id}
                  type="button"
                  onClick={() => openMeeting(meeting.id)}
                  className="flex flex-col gap-2 p-3 rounded-xl border border-gray-200 bg-white text-left hover:bg-gray-50 transition-colors"
                >
                  <span className="text-sm font-medium text-gray-900 line-clamp-2">{meeting.title}</span>
                  <span className="flex items-center justify-between text-xs text-gray-500">
                    <span className="flex items-center gap-1">
                      <CalendarDays className="w-3.5 h-3.5" />
                      {meeting.createdAt ? format(parseISO(meeting.createdAt), 'd MMM, HH:mm') : '—'}
                    </span>
                    <StageDots meeting={meeting} />
                  </span>
                </button>
              ))}
            </div>
          )}
        </Section>

        {/* Pipeline queue and background jobs; each renders nothing when idle.
            They size themselves to w-2/3 for the old layout, so stretch them. */}
        <div className="[&>div>div]:w-full [&>div>div]:max-w-none [&>div]:px-0 [&>div]:pt-0 space-y-4 empty:hidden">
          <BackgroundJobsPanel />
          <PendingWorkPanel />
        </div>
      </div>
    </div>
  );
}

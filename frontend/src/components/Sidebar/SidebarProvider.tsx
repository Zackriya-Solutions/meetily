'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { SummaryProcessResponse } from '@/types';
import type { MeetingStage, MeetingStages } from './meetingStages';
import { useTauriEvent } from '@/hooks/useTauriEvent';
import { errorMessage } from '@/lib/utils';



export interface SidebarItem extends Partial<MeetingStages> {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
}

export interface CurrentMeeting extends Partial<MeetingStages> {
  id: string;
  title: string;
  /** RFC 3339; absent for the placeholder "+ New Call" row. */
  createdAt?: string;
}

// Search result type for transcript search
interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
  timestamp: string;
}

/** Consecutive failed status reads (5 s apart) before reporting lost contact. */
const MAX_CONSECUTIVE_SUMMARY_READ_FAILURES = 6;

interface SummaryPoll {
  processId: string;
  timer: NodeJS.Timeout;
  inFlight: boolean;
}

interface SidebarContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  sidebarItems: SidebarItem[];
  isCollapsed: boolean;
  toggleCollapse: () => void;
  meetings: CurrentMeeting[];
  setMeetings: (meetings: CurrentMeeting[]) => void;
  isMeetingActive: boolean;
  setIsMeetingActive: (active: boolean) => void;
  handleRecordingToggle: () => void;
  searchTranscripts: (query: string) => Promise<void>;
  searchResults: TranscriptSearchResult[];
  isSearching: boolean;
  setServerAddress: (address: string) => void;
  serverAddress: string;
  transcriptServerAddress: string;
  setTranscriptServerAddress: (address: string) => void;
  // Summary polling management
  startSummaryPolling: (
    meetingId: string,
    processId: string,
    onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
  ) => void;
  stopSummaryPolling: (meetingId: string, processId?: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;
  /** The sidebar's "missing stage" chip; the home dashboard sets it too. */
  missingFilter: MeetingStage | null;
  setMissingFilter: (stage: MeetingStage | null) => void;
}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>({ id: 'intro-call', title: '+ New Call' });
  const [isCollapsed, setIsCollapsed] = useState(true);
  const [meetings, setMeetings] = useState<CurrentMeeting[]>([]);
  const [sidebarItems, setSidebarItems] = useState<SidebarItem[]>([]);
  const [isMeetingActive, setIsMeetingActive] = useState(false);
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [serverAddress, setServerAddress] = useState('');
  const [transcriptServerAddress, setTranscriptServerAddress] = useState('');
  const [missingFilter, setMissingFilter] = useState<MeetingStage | null>(null);
  const summaryPollsRef = React.useRef(new Map<string, SummaryPoll>());

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const pathname = usePathname();
  const router = useRouter();

  // meetings-changed often fires several times in a row and responses can arrive
  // out of order, so only the most recent request may update the list.
  const meetingsRequestIdRef = React.useRef(0);
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      const requestId = ++meetingsRequestIdRef.current;
      try {
        const meetings = await invoke('api_get_meetings') as Array<{
          id: string, title: string, created_at: string,
          transcribed: boolean, transcript_needed: boolean, summarized: boolean, obsidian_exported: boolean
        }>;
        if (requestId !== meetingsRequestIdRef.current) return;
        const transformedMeetings = meetings.map(({ obsidian_exported, transcript_needed, created_at, ...meeting }) => ({
          ...meeting,
          transcriptNeeded: transcript_needed,
          createdAt: created_at,
          obsidianExported: obsidian_exported,
        }));
        setMeetings(transformedMeetings);
        Analytics.trackBackendConnection(true);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        Analytics.trackBackendConnection(false, errorMessage(error));
      }
    }
  }, [serverAddress]);

  useEffect(() => {
    fetchMeetings();
  }, [serverAddress, fetchMeetings]);

  // Rust emits this wherever the list or a meeting's transcribed / summarized
  // / Obsidian-exported flags change (pipeline, summaries, imports, exports,
  // retranscription, rename, delete).
  useTauriEvent('meetings-changed', () => { void fetchMeetings(); });

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
      setTranscriptServerAddress('http://127.0.0.1:8178/stream');
    };
    fetchSettings();
  }, []);

  const baseItems: SidebarItem[] = [
    {
      id: 'meetings',
      title: 'Meeting Notes',
      type: 'folder' as const,
      children: [
        ...meetings.map(meeting => ({ ...meeting, type: 'file' as const }))
      ]
    },
  ];


  const toggleCollapse = React.useCallback(() => {
    setIsCollapsed(prev => !prev);
  }, []);

  // Update current meeting when on home page
  useEffect(() => {
    if (pathname === '/') {
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
    }
    setSidebarItems(baseItems);
  }, [pathname]);

  // Update sidebar items when meetings change
  useEffect(() => {
    setSidebarItems(baseItems);
  }, [meetings]);

  // Function to handle recording toggle from sidebar
  const handleRecordingToggle = React.useCallback(() => {
    if (!isRecording) {
      // Check if already on home page
      if (pathname === '/') {
        // Already on home - trigger recording directly via custom event
        console.log('Triggering recording from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      } else {
        // Not on home - navigate and use auto-start mechanism
        console.log('Navigating to home page with auto-start flag');
        sessionStorage.setItem('autoStartRecording', 'true');
        router.push('/');
      }

      // Track recording initiation from sidebar
      Analytics.trackButtonClick('start_recording', 'sidebar');
    } else {
      // Recording in progress - request a stop. The full stop flow (stop_recording
      // command, transcript flush, save, navigation) lives in the Home components.
      if (pathname === '/') {
        console.log('Triggering recording stop from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('stop-recording-from-sidebar'));
      } else {
        console.log('Navigating to home page with auto-stop flag');
        sessionStorage.setItem('autoStopRecording', 'true');
        router.push('/');
      }

      Analytics.trackButtonClick('stop_recording', 'sidebar');
    }
    // The actual recording start/stop is handled in the Home component
  }, [isRecording, pathname, router]);

  // Function to search through meeting transcripts. Responses can arrive out of
  // order, so only the most recent request may update the results.
  const searchRequestIdRef = React.useRef(0);
  const searchTranscripts = React.useCallback(async (query: string) => {
    const requestId = ++searchRequestIdRef.current;
    if (!query.trim()) {
      setSearchResults([]);
      setIsSearching(false);
      return;
    }

    try {
      setIsSearching(true);
      const results = await invoke('api_search_transcripts', { query }) as TranscriptSearchResult[];
      if (requestId !== searchRequestIdRef.current) return;
      setSearchResults(results);
    } catch (error) {
      if (requestId !== searchRequestIdRef.current) return;
      console.error('Error searching transcripts:', error);
      setSearchResults([]);
    } finally {
      if (requestId === searchRequestIdRef.current) {
        setIsSearching(false);
      }
    }
  }, []);

  // Summary polling management
  const stopSummaryPolling = React.useCallback((meetingId: string, processId?: string) => {
    const poll = summaryPollsRef.current.get(meetingId);
    if (!poll || (processId && poll.processId !== processId)) {
      return;
    }
    clearInterval(poll.timer);
    summaryPollsRef.current.delete(meetingId);
  }, []);

  const startSummaryPolling = React.useCallback((
    meetingId: string,
    processId: string,
    onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
  ) => {
    stopSummaryPolling(meetingId);
    let pollCount = 0;
    // The backend owns the job's lifecycle (and marks crashed or interrupted
    // jobs failed). A failed status read is a communication problem, not a
    // generation failure: keep polling through transient errors and only
    // report lost contact after a sustained run of them.
    let consecutiveReadFailures = 0;
    const reportPollingError = async (entry: SummaryPoll, message: string) => {
      try {
        await onUpdate({
          status: 'error',
          meetingName: null,
          meeting_id: meetingId,
          start: processId,
          end: null,
          data: null,
          error: message,
        });
      } catch (callbackError) {
        console.error('Failed to handle summary polling error:', callbackError);
      } finally {
        if (summaryPollsRef.current.get(meetingId) === entry) {
          stopSummaryPolling(meetingId, processId);
        }
      }
    };
    const poll = async () => {
      const entry = summaryPollsRef.current.get(meetingId);
      if (!entry || entry.processId !== processId || entry.inFlight) {
        return;
      }
      entry.inFlight = true;
      try {
        pollCount += 1;
        let result: SummaryProcessResponse;
        try {
          result = await invoke<SummaryProcessResponse>('api_get_summary', { meetingId });
          consecutiveReadFailures = 0;
        } catch (error) {
          if (summaryPollsRef.current.get(meetingId) !== entry) return;
          consecutiveReadFailures += 1;
          const detail = errorMessage(error);
          if (consecutiveReadFailures < MAX_CONSECUTIVE_SUMMARY_READ_FAILURES) {
            console.warn(
              `Summary status read failed (${consecutiveReadFailures}/${MAX_CONSECUTIVE_SUMMARY_READ_FAILURES}); retrying:`,
              detail
            );
            return;
          }
          await reportPollingError(
            entry,
            `Lost contact with the summary job (${detail}). It may still be running; reopen the meeting to check its status.`
          );
          return;
        }
        const current = summaryPollsRef.current.get(meetingId);
        if (current !== entry || result.start !== processId) {
          return;
        }
        try {
          await onUpdate(result);
        } catch (error) {
          if (summaryPollsRef.current.get(meetingId) !== entry) return;
          await reportPollingError(entry, errorMessage(error));
          return;
        }
        if (summaryPollsRef.current.get(meetingId) !== entry) return;
        if (
          result.status === 'completed'
          || result.status === 'error'
          || result.status === 'failed'
          || result.status === 'cancelled'
          || (result.status === 'idle' && pollCount > 1)
        ) {
          stopSummaryPolling(meetingId, processId);
        }
      } finally {
        const current = summaryPollsRef.current.get(meetingId);
        if (current === entry) {
          current.inFlight = false;
        }
      }
    };

    const timer = setInterval(() => void poll(), 5000);
    summaryPollsRef.current.set(meetingId, { processId, timer, inFlight: false });
  }, [stopSummaryPolling]);

  useEffect(() => () => {
    summaryPollsRef.current.forEach(({ timer }) => clearInterval(timer));
    summaryPollsRef.current.clear();
  }, []);



  // Memoized: this provider consumes useRecordingState(), so an inline object
  // literal here re-rendered every useSidebar() consumer on each 500ms
  // recording-state poll.
  const value = React.useMemo(() => ({
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    meetings,
    setMeetings,
    isMeetingActive,
    setIsMeetingActive,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    setServerAddress,
    serverAddress,
    transcriptServerAddress,
    setTranscriptServerAddress,
    startSummaryPolling,
    stopSummaryPolling,
    refetchMeetings: fetchMeetings,
    missingFilter,
    setMissingFilter,
  }), [
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    meetings,
    setMeetings,
    isMeetingActive,
    setIsMeetingActive,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    setServerAddress,
    serverAddress,
    transcriptServerAddress,
    setTranscriptServerAddress,
    startSummaryPolling,
    stopSummaryPolling,
    fetchMeetings,
    missingFilter,
  ]);

  return (
    <SidebarContext.Provider value={value}>
      {children}
    </SidebarContext.Provider>
  );
}

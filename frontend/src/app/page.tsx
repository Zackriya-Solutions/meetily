'use client';

import { useState, useEffect, useContext, useCallback, useRef } from 'react';
import { motion } from 'framer-motion';
import { Transcript, TranscriptUpdate, Summary, SummaryResponse } from '@/types';
import { EditableTitle } from '@/components/EditableTitle';
import { TranscriptView } from '@/components/TranscriptView';
import { RecordingControls } from '@/components/RecordingControls';
import { AISummary } from '@/components/AISummary';
import { DeviceSelection, SelectedDevices } from '@/components/DeviceSelection';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { TranscriptSettings, TranscriptModelProps } from '@/components/TranscriptSettings';
import { LanguageSelection } from '@/components/LanguageSelection';
// import { PermissionWarning } from '@/components/PermissionWarning';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { useNavigation } from '@/hooks/useNavigation';
import { useRouter } from 'next/navigation';
import type { CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import Analytics from '@/lib/analytics';
import { showRecordingNotification } from '@/lib/recordingNotification';
import { Button } from '@/components/ui/button';
import { Copy, GlobeIcon, Settings, Bot, Zap, X } from 'lucide-react';
import { ChatInterface } from '@/components/MeetingDetails/ChatInterface';
import { MicrophoneIcon } from '@heroicons/react/24/outline';
import { toast } from 'sonner';
import { ButtonGroup } from '@/components/ui/button-group';
import { apiUrl } from '@/lib/config';
import { authFetch, AuthError } from '@/lib/api';
import { recoveryService, PendingMeetingData } from '@/lib/transcriptRecovery';



interface ModelConfig {
  provider: 'ollama' | 'groq' | 'claude' | 'openrouter' | 'gemini' | 'openai';
  model: string;
  whisperModel: string;
}

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

interface OllamaModel {
  name: string;
  id: string;
  size: string;
  modified: string;
}

export default function Home() {

  const [transcripts, setTranscripts] = useState<Transcript[]>([]);
  const [showSummary, setShowSummary] = useState(false);
  const [summaryStatus, setSummaryStatus] = useState<SummaryStatus>('idle');
  const [barHeights, setBarHeights] = useState(['58%', '76%', '58%']);
  const [meetingTitle, setMeetingTitle] = useState('+ New Call');
  const [isEditingTitle, setIsEditingTitle] = useState(false);
  const [customPrompt, setCustomPrompt] = useState('');
  const [aiSummary, setAiSummary] = useState<Summary | null>({
    key_points: { title: "Key Points", blocks: [] },
    action_items: { title: "Action Items", blocks: [] },
    decisions: { title: "Decisions", blocks: [] },
    main_topics: { title: "Main Topics", blocks: [] }
  });
  const [summaryResponse, setSummaryResponse] = useState<SummaryResponse | null>(null);
  const [isCollapsed, setIsCollapsed] = useState(false);
  const [summaryError, setSummaryError] = useState<string | null>(null);
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'gemini',
    model: 'gemini-2.5-flash',
    whisperModel: 'large-v3'
  });
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'parakeet',
    model: 'parakeet-tdt-0.6b-v3-int8',
    apiKey: null
  });
  const [originalTranscript, setOriginalTranscript] = useState<string>('');
  const [models, setModels] = useState<OllamaModel[]>([]);
  const [error, setError] = useState<string>('');
  const [showModelSettings, setShowModelSettings] = useState(false);
  const [showErrorAlert, setShowErrorAlert] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const [showChunkDropWarning, setShowChunkDropWarning] = useState(false);
  const [chunkDropMessage, setChunkDropMessage] = useState('');
  const [isSavingTranscript, setIsSavingTranscript] = useState(false);
  const [isRecordingDisabled, setIsRecordingDisabled] = useState(false);
  const [selectedDevices, setSelectedDevices] = useState<SelectedDevices>({
    micDevice: null,
    systemDevice: null
  });
  const [showDeviceSettings, setShowDeviceSettings] = useState(false);
  const [showModelSelector, setShowModelSelector] = useState(false);
  const [modelSelectorMessage, setModelSelectorMessage] = useState('');
  const [showLanguageSettings, setShowLanguageSettings] = useState(false);
  const [selectedLanguage, setSelectedLanguage] = useState('auto-translate');
  const [isProcessingTranscript, setIsProcessingTranscript] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [showConfidenceIndicator, setShowConfidenceIndicator] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('showConfidenceIndicator');
      return saved !== null ? saved === 'true' : true;
    }
    return true;
  });

  // State for web audio recording
  // State for web audio recording
  const [isRecording, setIsRecording] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [isChatOpen, setIsChatOpen] = useState(false);

  // Recovery State
  const [showReauthModal, setShowReauthModal] = useState(false);
  const [pendingRecoveryId, setPendingRecoveryId] = useState<string | null>(null);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [isRestoredSession, setIsRestoredSession] = useState(false);
  const [resumeStartSignal, setResumeStartSignal] = useState(0);
  const [recordingElapsedSeconds, setRecordingElapsedSeconds] = useState(0);
  const [audioTimelineOffsetSeconds, setAudioTimelineOffsetSeconds] = useState(0);

  useEffect(() => {
    const checkRecoveries = async () => {
      if (typeof window !== 'undefined') {
        const launchParams = new URLSearchParams(window.location.search);
        if (launchParams.get('autoStart') === 'true') {
          return;
        }
      }

      const pending = await recoveryService.getAllPendingTranscripts();
      if (pending.length > 0) {
        const latest = pending[0];
        const ageMinutes = (Date.now() - latest.timestamp) / 1000 / 60;

        // If backup is fresh (< 5 mins), auto-restore seamlessly
        if (ageMinutes < 5) {
          console.log('[Recovery] Found fresh backup (< 5 mins), auto-restoring...');
          setMeetingTitle(latest.title);
          setTranscripts(latest.transcripts);
          const recoveredId = latest.sessionId || latest.meetingId;
          setPendingRecoveryId(recoveredId);
          setIsRestoredSession(true);

          // Don't set 'recovery' ID to avoid blocking recording controls
          // Just treat it as loaded state ready to append
          setCurrentSessionId(recoveredId);

          toast.success('Session Restored', {
            description: 'Your meeting context has been automatically restored.'
          });
        } else {
          // Old backup: Prompt user
          toast('Unsaved Transcripts Found', {
            description: `Found ${pending.length} unsaved meetings. Click to recover.`,
            action: {
              label: 'Recover',
              onClick: () => handleRecoverTranscripts(latest)
            },
            duration: 10000,
          });
        }
      }
    };
    checkRecoveries();
  }, []);

  const handleRecoverTranscripts = async (data: PendingMeetingData) => {
    setMeetingTitle(data.title);
    setTranscripts(data.transcripts);
    setCurrentMeeting({ id: 'recovery', title: data.title });
    const recoveredId = data.sessionId || data.meetingId;
    setPendingRecoveryId(recoveredId);
    setCurrentSessionId(recoveredId);
    toast.success('Restored unsaved meeting', { description: 'Please try saving again.' });
  };

  // Catch Me Up feature state
  const [isCatchUpOpen, setIsCatchUpOpen] = useState(false);
  const [catchUpSummary, setCatchUpSummary] = useState('');
  const [isCatchUpLoading, setIsCatchUpLoading] = useState(false);
  const [showCatchUpMenu, setShowCatchUpMenu] = useState(false);
  const [catchUpMinutes, setCatchUpMinutes] = useState<number | null>(null); // null = all, number = last N minutes
  const [customMinutesInput, setCustomMinutesInput] = useState('');


  // Permission check skipped as browser handles it

  const { currentMeeting, setCurrentMeeting, setMeetings, meetings, isMeetingActive, setIsMeetingActive, setIsRecording: setSidebarIsRecording, serverAddress, isCollapsed: sidebarCollapsed, refetchMeetings } = useSidebar();
  const handleNavigation = useNavigation('', ''); // Initialize with empty values
  const router = useRouter();

  // Ref for final buffer flush functionality
  const finalFlushRef = useRef<(() => void) | null>(null);

  // Ref to avoid stale closure issues with transcripts
  const transcriptsRef = useRef<Transcript[]>(transcripts);
  const meetingTitleRef = useRef<string>(meetingTitle);
  const currentSessionIdRef = useRef<string | null>(currentSessionId);
  const pendingRecoveryIdRef = useRef<string | null>(pendingRecoveryId);
  const audioTimelineOffsetRef = useRef<number>(audioTimelineOffsetSeconds);

  const isUserAtBottomRef = useRef<boolean>(true);

  // Ref for the transcript scrollable container
  const transcriptContainerRef = useRef<HTMLDivElement>(null);

  // Keep ref updated with current transcripts
  useEffect(() => {
    transcriptsRef.current = transcripts;
  }, [transcripts]);
  useEffect(() => {
    meetingTitleRef.current = meetingTitle;
  }, [meetingTitle]);
  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);
  useEffect(() => {
    pendingRecoveryIdRef.current = pendingRecoveryId;
  }, [pendingRecoveryId]);
  useEffect(() => {
    audioTimelineOffsetRef.current = audioTimelineOffsetSeconds;
  }, [audioTimelineOffsetSeconds]);

  const saveRecoverySnapshot = useCallback(async () => {
    const latestTranscripts = transcriptsRef.current;
    if (!latestTranscripts || latestTranscripts.length === 0) return;

    const recoveryId = currentSessionIdRef.current || pendingRecoveryIdRef.current || `recovery-${Date.now()}`;
    const defaultTemplate = localStorage.getItem('selectedTemplate') || 'standard_meeting';

    await recoveryService.savePendingTranscript({
      meetingId: recoveryId,
      title: meetingTitleRef.current || 'Untitled Meeting',
      transcripts: latestTranscripts,
      timestamp: Date.now(),
      templateId: defaultTemplate,
      sessionId: currentSessionIdRef.current
    });

    if (!pendingRecoveryIdRef.current) {
      pendingRecoveryIdRef.current = recoveryId;
      setPendingRecoveryId(recoveryId);
    }
  }, []);

  // Auto-save effect (stable timer; does not reset on each transcript append)
  useEffect(() => {
    if (!isRecording) return;

    const intervalId = setInterval(async () => {
      console.log('[AutoSave] Saving recovery backup...');
      try {
        await saveRecoverySnapshot();
      } catch (err) {
        console.warn('[AutoSave] Failed to save backup:', err);
      }
    }, 30000);

    return () => clearInterval(intervalId);
  }, [isRecording, saveRecoverySnapshot]);

  // Save shortly after transcript updates while recording.
  useEffect(() => {
    if (!isRecording || transcripts.length === 0) return;

    const timeoutId = setTimeout(() => {
      saveRecoverySnapshot().catch((err) => {
        console.warn('[AutoSave] Failed to save transcript-change snapshot:', err);
      });
    }, 1500);

    return () => clearTimeout(timeoutId);
  }, [isRecording, transcripts.length, saveRecoverySnapshot]);

  // Last-chance snapshot before reload/tab close.
  useEffect(() => {
    if (!isRecording) return;

    const handleBeforeUnload = () => {
      saveRecoverySnapshot().catch(() => {});
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    window.addEventListener('pagehide', handleBeforeUnload);

    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
      window.removeEventListener('pagehide', handleBeforeUnload);
    };
  }, [isRecording, saveRecoverySnapshot]);

  const parseTranscriptTimestampMs = useCallback((timestamp?: string): number | null => {
    if (!timestamp) return null;

    // Handle HH:MM:SS local-style strings
    if (/^\d{2}:\d{2}:\d{2}$/.test(timestamp)) {
      const [hours, minutes, seconds] = timestamp.split(':').map(Number);
      const now = new Date();
      const parsed = new Date(
        now.getFullYear(),
        now.getMonth(),
        now.getDate(),
        hours,
        minutes,
        seconds,
        0
      );
      return parsed.getTime();
    }

    const parsed = new Date(timestamp).getTime();
    return Number.isNaN(parsed) ? null : parsed;
  }, []);

  const getTranscriptElapsedSeconds = useCallback((items: Transcript[]): number => {
    if (!items.length) return 0;

    const timestampMs = items
      .map((t) => parseTranscriptTimestampMs(t.timestamp))
      .filter((t): t is number => t !== null)
      .sort((a, b) => a - b);

    if (timestampMs.length >= 2) {
      const elapsed = Math.floor((timestampMs[timestampMs.length - 1] - timestampMs[0]) / 1000);
      if (elapsed > 0) return elapsed;
    }

    const maxAudioEnd = items.reduce((max, t) => {
      const end = t.audio_end_time ?? t.audio_start_time ?? 0;
      return end > max ? end : max;
    }, 0);

    return Math.floor(maxAudioEnd);
  }, [parseTranscriptTimestampMs]);

  // Keep elapsed duration in sync while recording, including silent intervals.
  useEffect(() => {
    if (!isRecording || isPaused) return;

    const intervalId = setInterval(() => {
      setRecordingElapsedSeconds((prev) => prev + 1);
    }, 1000);

    return () => clearInterval(intervalId);
  }, [isRecording, isPaused]);

  // Keep a reasonable baseline when loading recovered transcripts before resuming.
  useEffect(() => {
    if (isRecording) return;
    const inferred = getTranscriptElapsedSeconds(transcripts);
    setRecordingElapsedSeconds((prev) => (inferred > prev ? inferred : prev));
  }, [transcripts, isRecording, getTranscriptElapsedSeconds]);

  // Smart auto-scroll: Track user scroll position
  useEffect(() => {
    const handleScroll = () => {
      const container = transcriptContainerRef.current;
      if (!container) return;

      const { scrollTop, scrollHeight, clientHeight } = container;
      const isAtBottom = scrollTop + clientHeight >= scrollHeight - 10; // 10px tolerance
      isUserAtBottomRef.current = isAtBottom;
    };

    const container = transcriptContainerRef.current;
    if (container) {
      container.addEventListener('scroll', handleScroll);
      return () => container.removeEventListener('scroll', handleScroll);
    }
  }, []);

  // Auto-scroll when transcripts change (only if user is at bottom)
  useEffect(() => {
    // Only auto-scroll if user was at the bottom before new content
    if (isUserAtBottomRef.current && transcriptContainerRef.current) {
      // Wait for Framer Motion animation to complete (150ms) before scrolling
      // This ensures scrollHeight includes the full rendered height of the new transcript
      const scrollTimeout = setTimeout(() => {
        const container = transcriptContainerRef.current;
        if (container) {
          container.scrollTo({
            top: container.scrollHeight,
            behavior: 'smooth'
          });
        }
      }, 150); // Match Framer Motion transition duration

      return () => clearTimeout(scrollTimeout);
    }
  }, [transcripts]);

  const modelOptions: Record<ModelConfig['provider'], string[]> = {
    ollama: models.map(model => model.name),
    claude: ['claude-3-5-sonnet-latest'],
    groq: ['llama-3.3-70b-versatile'],
    openrouter: [],
    gemini: ['gemini-2.5-flash', 'gemini-2.5-pro', 'gemini-2.0-flash'],
    openai: ['gpt-4o', 'gpt-4-turbo'],
  };

  useEffect(() => {
    if (models.length > 0 && modelConfig.provider === 'ollama') {
      setModelConfig(prev => ({
        ...prev,
        model: models[0].name
      }));
    }
  }, [models]);

  const whisperModels = [
    'tiny',
    'tiny.en',
    'tiny-q5_1',
    'tiny.en-q5_1',
    'tiny-q8_0',
    'base',
    'base.en',
    'base-q5_1',
    'base.en-q5_1',
    'base-q8_0',
    'small',
    'small.en',
    'small.en-tdrz',
    'small-q5_1',
    'small.en-q5_1',
    'small-q8_0',
    'medium',
    'medium.en',
    'medium-q5_0',
    'medium.en-q5_0',
    'medium-q8_0',
    'large-v1',
    'large-v2',
    'large-v2-q5_0',
    'large-v2-q8_0',
    'large-v3',
    'large-v3-q5_0',
    'large-v3-turbo',
    'large-v3-turbo-q5_0',
    'large-v3-turbo-q8_0'
  ];

  useEffect(() => {
    // Track page view
    Analytics.trackPageView('home');
  }, []);

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const savedConfig = localStorage.getItem('transcript_config');
        if (savedConfig) {
          const config = JSON.parse(savedConfig);
          console.log('Loaded saved transcript config from localStorage:', config);
          setTranscriptModelConfig(config);
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, []);

  useEffect(() => {
    setCurrentMeeting({ id: 'intro-call', title: meetingTitle });

  }, [meetingTitle, setCurrentMeeting]);





  useEffect(() => {
    if (isRecording) {
      const interval = setInterval(() => {
        setBarHeights(prev => {
          const newHeights = [...prev];
          newHeights[0] = Math.random() * 20 + 10 + 'px';
          newHeights[1] = Math.random() * 20 + 10 + 'px';
          newHeights[2] = Math.random() * 20 + 10 + 'px';
          return newHeights;
        });
      }, 300);

      return () => clearInterval(interval);
    }
  }, [isRecording]);

  // Update sidebar recording state
  useEffect(() => {
    setSidebarIsRecording(isRecording);
    return () => {
      // Prevent stale sidebar "recording" UI if Home unmounts mid-transition.
      setSidebarIsRecording(false);
    };
  }, [isRecording, setSidebarIsRecording]);

  // Handle receiving transcript updates from RecordingControls
  const handleTranscriptReceived = useCallback((newTranscript: TranscriptUpdate) => {
    // Deduplicate by sequence_id
    setTranscripts(prev => {
      // Check if we already have this transcript
      if (prev.some(t => t.sequence_id === newTranscript.sequence_id)) {
        return prev;
      }

      const transcriptData: Transcript = {
        id: `${Date.now()}-${prev.length}`,
        text: newTranscript.text,
        timestamp: newTranscript.timestamp,
        sequence_id: newTranscript.sequence_id,
        is_partial: newTranscript.is_partial,
        // Optional fields
        chunk_start_time: newTranscript.chunk_start_time,
        confidence: newTranscript.confidence,
        audio_start_time: newTranscript.audio_start_time,
        audio_end_time: newTranscript.audio_end_time,
        duration: newTranscript.duration,
      };

      return [...prev, transcriptData].sort((a, b) => (a.sequence_id || 0) - (b.sequence_id || 0));
    });

    // Auto-scroll logic is handled by the existing useEffect on [transcripts]
  }, []);

  // Sync transcript history and meeting name from backend on reload
  // This fixes the issue where reloading during active recording causes state desync








  /*
  useEffect(() => {
    const loadModels = async () => {
      try {
        const response = await fetch('http://localhost:11434/api/tags', {
          method: 'GET',
          headers: {
            'Content-Type': 'application/json',
          },
        });

        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }

        const data = await response.json();
        const modelList = data.models.map((model: any) => ({
          name: model.name,
          id: model.model,
          size: formatSize(model.size),
          modified: model.modified_at
        }));
        setModels(modelList);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load Ollama models');
        console.error('Error loading models:', err);
      }
    };

    loadModels();
  }, []);
  */

  /*
  const formatSize = (size: number): string => {
    if (size < 1024) {
      return `${size} B`;
    } else if (size < 1024 * 1024) {
      return `${(size / 1024).toFixed(1)} KB`;
    } else if (size < 1024 * 1024 * 1024) {
      return `${(size / (1024 * 1024)).toFixed(1)} MB`;
    } else {
      return `${(size / (1024 * 1024 * 1024)).toFixed(1)} GB`;
    }
  };
  */

  const handleRecordingStart = async () => {
    try {
      console.log('handleRecordingStart called - setting up meeting title and state');

      const now = new Date();
      const day = String(now.getDate()).padStart(2, '0');
      const month = String(now.getMonth() + 1).padStart(2, '0');
      const year = String(now.getFullYear()).slice(-2);
      const hours = String(now.getHours()).padStart(2, '0');
      const minutes = String(now.getMinutes()).padStart(2, '0');
      const seconds = String(now.getSeconds()).padStart(2, '0');
      const randomTitle = `Meeting ${day}_${month}_${year}_${hours}_${minutes}_${seconds}`;

      // Only set new title if we are NOT recovering/resuming
      // If pendingRecoveryId exists, we want to keep the restored title
      // If currentSessionId exists, we are resuming an active session
      if (!pendingRecoveryId && !currentSessionId && meetingTitle === '+ New Call') {
        setMeetingTitle(randomTitle);
      }

      // Update state
      console.log('Setting recording state to true');
      setIsRecording(true);
      setIsPaused(false);

      const inferredElapsed = Math.max(
        recordingElapsedSeconds,
        getTranscriptElapsedSeconds(transcripts)
      );

      // If this is a restored session or we have existing transcripts, we are RESUMING
      // We must ensure new transcripts don't collide with old ones
      if (transcripts.length > 0) {
        // Ensure earliest resumed transcript chunks use the resumed timeline.
        audioTimelineOffsetRef.current = inferredElapsed;
        setRecordingElapsedSeconds(inferredElapsed);
        setAudioTimelineOffsetSeconds(inferredElapsed);
        if (currentSessionId && !pendingRecoveryId) {
          console.log('[Recording] Resuming existing session with ID:', currentSessionId);
          setIsRestoredSession(true); // Keep strictly true to indicate continuation
        } else {
          console.log('[Recording] Starting new session. Archiving previous transcripts...');
          // Invalidate sequence IDs of existing transcripts to prevent collision with new stream (which starts at 0)
          setTranscripts(prev => prev.map(t => ({
            ...t,
            sequence_id: -1 // Mark as "historical" so handleTranscriptReceived won't dedup against new 0
          })));
          setIsRestoredSession(false);
        }
      } else {
        setTranscripts([]); // Clear previous transcripts only if starting fresh
        setRecordingElapsedSeconds(0);
        setAudioTimelineOffsetSeconds(0);
      }

      setIsMeetingActive(true);
      Analytics.trackButtonClick('start_recording', 'home_page');

      // Show recording notification if enabled
      await showRecordingNotification();
    } catch (error) {
      console.error('Failed to start recording:', error);
      alert('Failed to start recording. Check console for details.');
      setIsRecording(false);
      Analytics.trackButtonClick('start_recording_error', 'home_page');
    }
  };

  // Check for autoStartRecording flag and start recording automatically
  useEffect(() => {
    const checkAutoStartRecording = async () => {
      if (typeof window !== 'undefined') {
        const shouldAutoStart = sessionStorage.getItem('autoStartRecording');
        const urlParams = new URLSearchParams(window.location.search);
        const shouldAutoStartFromUrl = urlParams.get('autoStart') === 'true';

        if ((shouldAutoStart === 'true' || shouldAutoStartFromUrl) && !isRecording && !isMeetingActive) {
          console.log('Auto-starting recording from navigation/new-tab launch...');
          sessionStorage.removeItem('autoStartRecording'); // Clear the flag
          if (shouldAutoStartFromUrl) {
            urlParams.delete('autoStart');
            urlParams.delete('source');
            const nextQuery = urlParams.toString();
            window.history.replaceState(
              {},
              '',
              `${window.location.pathname}${nextQuery ? `?${nextQuery}` : ''}`
            );
          }
          setResumeStartSignal((v) => v + 1);
        }
      }
    };

    checkAutoStartRecording();
  }, [isRecording, isMeetingActive]);


  // Stop recording and save audio


  const handleWebAudioRecordingStop = async () => {
    console.log('💾 [Web Audio] Saving meeting to database...');
    setSummaryStatus('processing');
    setIsSavingTranscript(true);

    try {
      const freshTranscripts = transcriptsRef.current;

      if (freshTranscripts.length === 0) {
        console.warn('No transcripts to save');
        toast.error('No transcripts to save', {
          description: 'Recording was too short or no speech was detected.'
        });
        return;
      }

      console.log(`💾 Saving ${freshTranscripts.length} transcripts via HTTP API...`);

      // Get default template from localStorage or use 'standard_meeting'
      const defaultTemplate = localStorage.getItem('selectedTemplate') || 'standard_meeting';

      // Call backend API directly - backend will automatically generate notes based on template
      const response = await authFetch('/save-transcript', {
        method: 'POST',
        // headers: { 'Content-Type': 'application/json' }, // authFetch handles Content-Type
        body: JSON.stringify({
          meeting_title: meetingTitle || 'Web Audio Meeting',
          transcripts: freshTranscripts.map((t, index) => ({
            id: t.id || `transcript-${Date.now()}-${Math.random().toString(36).substring(2, 9)}-${index}`,
            text: t.text,
            timestamp: t.timestamp,
            audio_start_time: t.audio_start_time || 0,
            audio_end_time: t.audio_end_time || 0,
            duration: t.duration || 0,
          })),
          folder_path: null,
          template_id: defaultTemplate,
          meeting_id: currentSessionId || pendingRecoveryId || undefined,
          session_id: currentSessionId,
        }),
        preventLogout: true
      });

      if (response.ok) {
        // Clear session ID after successful save
        setCurrentSessionId(null);
      }

      if (!response.ok) {
        throw new Error(`Failed to save meeting: ${response.statusText}`);
      }

      const data = await response.json();
      const meetingId = data.meeting_id;

      console.log('✅ [Web Audio] Meeting saved with ID:', meetingId);

      // Store transcript for LLM post-processing
      const fullTranscript = freshTranscripts.map(t => t.text).join('\n');
      setOriginalTranscript(fullTranscript);

      // Cleanup pending recovery if exists
      if (pendingRecoveryId) {
        await recoveryService.deletePendingTranscript(pendingRecoveryId);
        setPendingRecoveryId(null);
      }

      // Update UI
      await refetchMeetings();
      setCurrentMeeting({
        id: meetingId,
        title: meetingTitle || 'Web Audio Meeting'
      });

      toast.success('Recording saved! Generating meeting notes...', {
        description: `Notes are being generated automatically based on ${defaultTemplate} template.`,
        duration: 5000,
      });

      // Notes are now generated automatically by the backend
      // Navigate to meeting details page after a short delay
      setTimeout(() => {
        router.push(`/meeting-details?id=${meetingId}`);
        Analytics.trackPageView('meeting_details');
      }, 1500);

    } catch (error) {
      console.error('❌ [Web Audio] Failed to save meeting:', error);

      if (error instanceof AuthError) {
        // Save to recovery
        const freshTranscripts = transcriptsRef.current;
        const recoveryId = currentSessionId || pendingRecoveryId || `recovery-${Date.now()}`;
        const defaultTemplate = localStorage.getItem('selectedTemplate') || 'standard_meeting';

        await recoveryService.savePendingTranscript({
          meetingId: recoveryId,
          title: meetingTitle || 'Untitled Meeting',
          transcripts: freshTranscripts,
          timestamp: Date.now(),
          templateId: defaultTemplate
        });

        setPendingRecoveryId(recoveryId);
        setShowReauthModal(true);
        toast.error('Session Expired', {
          description: 'Your session expired while saving. Please log in again to save your meeting.',
          duration: 10000
        });
      } else {
        toast.error('Failed to save recording', {
          description: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    } finally {
      setSummaryStatus('idle');
      setIsSavingTranscript(false);
      setIsProcessingTranscript(false);
      setIsRecordingDisabled(false);
      setIsStopping(false);
      setIsMeetingActive(false);
      setSidebarIsRecording(false);
    }
  };

  const handleRecordingStop = async (success: boolean = true) => {
    // Immediately update UI state to reflect that recording has stopped
    setIsRecording(false);
    setIsPaused(false);
    setIsRecordingDisabled(false);
    setIsMeetingActive(false);
    setSidebarIsRecording(false);

    if (success) {
      await handleWebAudioRecordingStop();
    }
  };

  const handleTranscriptUpdate = (update: any) => {
    console.log('🎯 handleTranscriptUpdate called with:', {
      sequence_id: update.sequence_id,
      text: update.text.substring(0, 50) + '...',
      timestamp: update.timestamp,
      is_partial: update.is_partial
    });

    const adjustedAudioStart =
      typeof update.audio_start_time === 'number'
        ? update.audio_start_time + audioTimelineOffsetRef.current
        : undefined;
    const adjustedAudioEnd =
      typeof update.audio_end_time === 'number'
        ? update.audio_end_time + audioTimelineOffsetRef.current
        : undefined;

    const newTranscript = {
      id: update.sequence_id ? update.sequence_id.toString() : Date.now().toString(),
      text: update.text,
      timestamp: update.timestamp,
      sequence_id: update.sequence_id || 0,
      audio_start_time: adjustedAudioStart,
      audio_end_time: adjustedAudioEnd,
      duration: update.duration
    };

    setTranscripts(prev => {
      console.log('📊 Current transcripts count before update:', prev.length);

      // Check if this transcript already exists
      const exists = prev.some(
        t => t.text === update.text && t.timestamp === update.timestamp
      );
      if (exists) {
        console.log('🚫 Duplicate transcript detected, skipping:', update.text.substring(0, 30) + '...');
        return prev;
      }

      // Add new transcript and sort by audio_start_time to fix ordering
      const updated = [...prev, newTranscript];
      const sorted = updated.sort((a, b) => {
        // Primary sort: audio timestamp (if available)
        if (a.audio_start_time !== undefined && b.audio_start_time !== undefined) {
          // If timestamps are essentially equal (within 100ms), rely on sequence ID
          if (Math.abs(a.audio_start_time - b.audio_start_time) < 0.1) {
            return (a.sequence_id || 0) - (b.sequence_id || 0);
          }
          return a.audio_start_time - b.audio_start_time;
        }
        // Fallback: sequence ID (arrival time)
        return (a.sequence_id || 0) - (b.sequence_id || 0);
      });

      console.log('✅ Added new transcript. New count:', sorted.length);
      console.log('📝 Latest transcript:', {
        id: newTranscript.id,
        text: newTranscript.text.substring(0, 30) + '...',
        sequence_id: newTranscript.sequence_id
      });

      return sorted;
    });
  };

  const generateAISummary = useCallback(async (prompt: string = '') => {
    setSummaryStatus('processing');
    setSummaryError(null);

    try {
      const fullTranscript = transcripts.map(t => t.text).join('\n');
      if (!fullTranscript.trim()) {
        throw new Error('No transcript text available. Please add some text first.');
      }

      // Store the original transcript for regeneration
      setOriginalTranscript(fullTranscript);

      console.log('Generating summary for transcript length:', fullTranscript.length);

      // Process transcript
      console.log('Processing transcript...');

      const processResponse = await authFetch('/process-transcript', {
        method: 'POST',
        // headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          text: fullTranscript,
          meeting_id: currentMeeting?.id || 'new-meeting', // Ensure meeting_id is passed if available
          model: modelConfig.provider,
          modelName: modelConfig.model,
          chunkSize: 40000,
          overlap: 1000,
          customPrompt: prompt,
        })
      });

      if (!processResponse.ok) throw new Error('Failed to start processing');

      const result = await processResponse.json();
      const process_id = result.process_id;
      console.log('Process ID:', process_id);


      // Poll for summary status
      const pollInterval = setInterval(async () => {
        try {

          const summaryResponse = await authFetch(`/get-summary/${process_id}`);
          const result = await summaryResponse.json();
          console.log('Summary status:', result);

          if (result.status === 'error') {
            setSummaryError(result.error || 'Unknown error');
            setSummaryStatus('error');
            clearInterval(pollInterval);
            return;
          }

          if (result.status === 'completed' && result.data) {
            clearInterval(pollInterval);

            // Remove MeetingName from data before formatting
            const { MeetingName, ...summaryData } = result.data;

            // Update meeting title if available
            if (MeetingName) {
              setMeetingTitle(MeetingName);
            }

            // Format the summary data with consistent styling
            const formattedSummary = Object.entries(summaryData).reduce((acc: Summary, [key, section]: [string, any]) => {
              acc[key] = {
                title: section.title,
                blocks: section.blocks.map((block: any) => ({
                  ...block,
                  // type: 'bullet',
                  color: 'default',
                  content: block.content.trim() // Remove trailing newlines
                }))
              };
              return acc;
            }, {} as Summary);

            setAiSummary(formattedSummary);
            setSummaryStatus('completed');
          }
        } catch (error) {
          console.error('Failed to get summary status:', error);
          if (error instanceof Error) {
            setSummaryError(`Failed to get summary status: ${error.message}`);
          } else {
            setSummaryError('Failed to get summary status: Unknown error');
          }
          setSummaryStatus('error');
          clearInterval(pollInterval);
        }
      }, 3000); // Poll every 3 seconds

      // Cleanup interval on component unmount
      return () => clearInterval(pollInterval);

    } catch (error) {
      console.error('Failed to generate summary:', error);
      if (error instanceof Error) {
        setSummaryError(`Failed to generate summary: ${error.message}`);
      } else {
        setSummaryError('Failed to generate summary: Unknown error');
      }
      setSummaryStatus('error');
    }
  }, [transcripts, modelConfig, serverAddress]);

  const handleSummary = useCallback((summary: any) => {
    setAiSummary(summary);
  }, []);

  const handleSummaryChange = (newSummary: Summary) => {
    console.log('Summary changed:', newSummary);
    setAiSummary(newSummary);
  };

  const handleTitleChange = (newTitle: string) => {
    setMeetingTitle(newTitle);
    setCurrentMeeting({ id: 'intro-call', title: newTitle });
  };

  const getSummaryStatusMessage = (status: SummaryStatus) => {
    switch (status) {
      case 'idle':
        return 'Ready to generate summary';
      case 'processing':
        return isRecording ? 'Processing transcript...' : 'Finalizing transcription...';
      case 'summarizing':
        return 'Generating AI summary...';
      case 'regenerating':
        return 'Regenerating AI summary...';
      case 'completed':
        return 'Summary generated successfully!';
      case 'error':
        return summaryError || 'An error occurred';
      default:
        return '';
    }
  };

  const handleDownloadTranscript = async () => {
    try {
      // Create transcript object with metadata
      const transcriptData = {
        title: meetingTitle,
        timestamp: new Date().toISOString(),
        transcripts: transcripts
      };

      // Generate filename
      const sanitizedTitle = meetingTitle.replace(/[^a-zA-Z0-9]/g, '_');
      const filename = `${sanitizedTitle}_transcript.json`;

      // Create blob and download link
      const blob = new Blob([JSON.stringify(transcriptData, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);

      console.log('Transcript downloaded successfully');
    } catch (error) {
      console.error('Failed to download transcript:', error);
      alert('Failed to download transcript. Please try again.');
    }
  };

  const handleUploadTranscript = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    try {
      const text = await file.text();
      const data = JSON.parse(text);

      // Validate the uploaded file structure
      if (!data.transcripts || !Array.isArray(data.transcripts)) {
        throw new Error('Invalid transcript file format');
      }

      // Update state with uploaded data
      setMeetingTitle(data.title || 'Uploaded Transcript');
      setTranscripts(data.transcripts);

      // Generate summary for the uploaded transcript
      handleSummary(data.transcripts);
    } catch (error) {
      console.error('Error uploading transcript:', error);
      alert('Failed to upload transcript. Please make sure the file format is correct.');
    }
  };

  const handleRegenerateSummary = useCallback(async () => {
    if (!originalTranscript.trim()) {
      console.error('No original transcript available for regeneration');
      return;
    }

    setSummaryStatus('regenerating');
    setSummaryError(null);

    try {
      console.log('Regenerating summary with original transcript...');

      // Process transcript
      console.log('Processing transcript...');

      const processResponse = await authFetch('/process-transcript', {
        method: 'POST',
        // headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          text: originalTranscript,
          meeting_id: currentMeeting?.id, // Important for security and ownership
          model: modelConfig.provider,
          modelName: modelConfig.model,
          chunkSize: 40000,
          overlap: 1000,
        })
      });

      if (!processResponse.ok) throw new Error('Failed to start processing');

      const result = await processResponse.json();
      const process_id = result.process_id;
      console.log('Process ID:', process_id);

      // Poll for summary status
      const pollInterval = setInterval(async () => {
        try {

          const summaryResponse = await authFetch(`/get-summary/${process_id}`);
          const result = await summaryResponse.json();
          console.log('Summary status:', result);

          if (result.status === 'error') {
            setSummaryError(result.error || 'Unknown error');
            setSummaryStatus('error');
            clearInterval(pollInterval);
            return;
          }

          if (result.status === 'completed' && result.data) {
            clearInterval(pollInterval);

            // Remove MeetingName from data before formatting
            const { MeetingName, ...summaryData } = result.data;

            // Update meeting title if available
            if (MeetingName) {
              setMeetingTitle(MeetingName);
            }

            // Format the summary data with consistent styling
            const formattedSummary = Object.entries(summaryData).reduce((acc: Summary, [key, section]: [string, any]) => {
              acc[key] = {
                title: section.title,
                blocks: section.blocks.map((block: any) => ({
                  ...block,
                  // type: 'bullet',
                  color: 'default',
                  content: block.content.trim()
                }))
              };
              return acc;
            }, {} as Summary);

            setAiSummary(formattedSummary);
            setSummaryStatus('completed');
          } else if (result.status === 'error') {
            clearInterval(pollInterval);
            throw new Error(result.error || 'Failed to generate summary');
          }
        } catch (error) {
          clearInterval(pollInterval);
          console.error('Failed to get summary status:', error);
          if (error instanceof Error) {
            setSummaryError(error.message);
          } else {
            setSummaryError('An unexpected error occurred');
          }
          setSummaryStatus('error');
          setAiSummary(null);
        }
      }, 1000);

      return () => clearInterval(pollInterval);
    } catch (error) {
      console.error('Failed to regenerate summary:', error);
      if (error instanceof Error) {
        setSummaryError(error.message);
      } else {
        setSummaryError('An unexpected error occurred');
      }
      setSummaryStatus('error');
      setAiSummary(null);
    }
  }, [originalTranscript, modelConfig, serverAddress]);

  const handleCopyTranscript = useCallback(async () => {
    // Format timestamps as recording-relative [MM:SS] instead of wall-clock time
    const formatTime = (seconds: number | undefined): string => {
      if (seconds === undefined) return '[00:00]';
      const totalSecs = Math.floor(seconds);
      const mins = Math.floor(totalSecs / 60);
      const secs = totalSecs % 60;
      return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
    };

    const fullTranscript = transcripts
      .map(t => `${formatTime(t.audio_start_time)} ${t.text}`)
      .join('\n');
    try {
      await navigator.clipboard.writeText(fullTranscript);
      toast.success("Transcript copied to clipboard");
    } catch (err) {
      console.error('Failed to copy transcript:', err);
      toast.error('Failed to copy transcript to clipboard');
    }
  }, [transcripts]);

  // Handle Catch Me Up - get quick summary of meeting so far
  // Get meeting duration in minutes for time range validation
  const getMeetingDurationMinutes = useCallback(() => {
    const elapsedSeconds = Math.max(
      recordingElapsedSeconds,
      getTranscriptElapsedSeconds(transcripts)
    );
    return Math.ceil(elapsedSeconds / 60);
  }, [transcripts, recordingElapsedSeconds, getTranscriptElapsedSeconds]);

  const handleCatchUp = useCallback(async (minutes: number | null = null) => {
    if (!transcripts.length && minutes === null) {
      toast.error("No transcript yet to catch up on");
      return;
    }

    setShowCatchUpMenu(false);
    setIsCatchUpOpen(true);
    setIsCatchUpLoading(true);
    setCatchUpSummary('');
    setCatchUpMinutes(minutes);

    try {
      // Filter transcripts by time range if specified
      let filteredTranscripts = transcripts;
      const nowMs = Date.now();
      const windowStartMs = minutes !== null ? nowMs - (minutes * 60 * 1000) : null;

      if (minutes !== null && windowStartMs !== null && transcripts.length > 0) {
        filteredTranscripts = transcripts.filter((t) => {
          const ts = parseTranscriptTimestampMs(t.timestamp);
          return ts !== null && ts >= windowStartMs;
        });
      }

      // Safety limit: only take last 1000 transcripts to avoid payload too large errors
      if (filteredTranscripts.length > 1000) {
        console.warn('[CatchUp] Limiting to last 1000 transcripts to avoid payload issues');
        filteredTranscripts = filteredTranscripts.slice(-1000);
      }

      const transcriptEntries = filteredTranscripts.map((t) => ({
        text: t.text,
        timestamp: t.timestamp,
        audio_start_time: t.audio_start_time
      }));

      // Optimization: Limit the number of transcripts sent to prevent payload size issues
      // 1000 transcripts is roughly 1.5 - 2 hours of meeting data
      const MAX_CATCHUP_TRANSCRIPTS = 1000;
      const limitedTranscriptEntries = transcriptEntries.length > MAX_CATCHUP_TRANSCRIPTS
        ? transcriptEntries.slice(-MAX_CATCHUP_TRANSCRIPTS)
        : transcriptEntries;

      if (transcriptEntries.length > MAX_CATCHUP_TRANSCRIPTS) {
        console.warn(`[CatchUp] Truncating ${transcriptEntries.length} transcripts to latest ${MAX_CATCHUP_TRANSCRIPTS} to prevent payload size issues.`);
      }

      const timeLabel = minutes ? `last ${minutes} minutes` : 'entire meeting';
      console.log(`[CatchUp] Summarizing ${timeLabel}: ${limitedTranscriptEntries.length} transcripts`);

      // The catch-up endpoint currently only supports gemini and groq
      const supportedProviders = ['gemini', 'groq'];
      let provider = modelConfig?.provider || 'gemini';
      let modelName = modelConfig?.model || 'gemini-2.5-flash';

      if (!supportedProviders.includes(provider)) {
        console.warn(`[CatchUp] Unsupported provider "${provider}" selected. Falling back to Gemini.`);
        provider = 'gemini';
        modelName = 'gemini-2.5-flash';
      }

      const response = await authFetch('/catch-up', {
        method: 'POST',
        body: JSON.stringify({
          transcripts: limitedTranscriptEntries,
          model: provider,
          model_name: modelName,
          window_minutes: minutes,
          window_start_iso: windowStartMs ? new Date(windowStartMs).toISOString() : null,
          window_end_iso: new Date(nowMs).toISOString(),
          meeting_elapsed_seconds: Math.max(
            recordingElapsedSeconds,
            getTranscriptElapsedSeconds(transcripts)
          )
        })
      });

      if (!response.ok) {
        throw new Error(`Failed to get catch-up: ${response.statusText}`);
      }

      const contentType = response.headers.get('content-type') || '';
      if (contentType.includes('application/json')) {
        const data = await response.json();
        setCatchUpSummary(data.summary || data.error || 'No catch-up available for this window.');
        return;
      }

      // Stream the response
      const reader = response.body?.getReader();
      if (!reader) throw new Error('No response body');

      const decoder = new TextDecoder();
      let summary = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        const chunk = decoder.decode(value, { stream: true });
        summary += chunk;
        setCatchUpSummary(summary);
      }
    } catch (error) {
      console.error('Catch-up error:', error);
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      toast.error('Catch Up Failed', {
        description: `Could not generate summary: ${errorMessage}. Please try again.`
      });
      setCatchUpSummary('Error getting catch-up summary. Please try again.');
    } finally {
      setIsCatchUpLoading(false);
    }
  }, [transcripts, parseTranscriptTimestampMs, modelConfig, recordingElapsedSeconds, getTranscriptElapsedSeconds]);


  const handleGenerateSummary = useCallback(async () => {
    if (!transcripts.length) {
      console.log('No transcripts available for summary');
      return;
    }

    try {
      await generateAISummary(customPrompt);
    } catch (error) {
      console.error('Failed to generate summary:', error);
      if (error instanceof Error) {
        setSummaryError(error.message);
      } else {
        setSummaryError('Failed to generate summary: Unknown error');
      }
    }
  }, [transcripts, generateAISummary]);

  // Handle transcript configuration save
  const handleSaveTranscriptConfig = async (config: TranscriptModelProps) => {
    try {
      console.log('[HomePage] Saving transcript config to localStorage:', config);
      localStorage.setItem('transcript_config', JSON.stringify(config));
      console.log('[HomePage] ✅ Successfully saved transcript config');
    } catch (error) {
      console.error('[HomePage] ❌ Failed to save transcript config:', error);
    }
  };

  // Handle confidence indicator toggle
  const handleConfidenceToggle = (checked: boolean) => {
    setShowConfidenceIndicator(checked);
    if (typeof window !== 'undefined') {
      localStorage.setItem('showConfidenceIndicator', checked.toString());
    }
    // Trigger a custom event to notify other components
    window.dispatchEvent(new CustomEvent('confidenceIndicatorChanged', { detail: checked }));
  };



  const isSummaryLoading = summaryStatus === 'processing' || summaryStatus === 'summarizing' || summaryStatus === 'regenerating';

  const isProcessingStop = summaryStatus === 'processing' || isProcessingTranscript
  useEffect(() => {
    // Honor saved model settings from backend (including OpenRouter)
    const fetchModelConfig = async () => {
      try {
        const response = await authFetch('/get-model-config');
        if (response.ok) {
          const data = await response.json();
          if (data && data.provider) {
            setModelConfig(prev => ({
              ...prev,
              provider: data.provider,
              model: data.model || prev.model,
              whisperModel: data.whisperModel || prev.whisperModel,
            }));
          }
        }
      } catch (error) {
        console.error('Failed to fetch saved model config in page.tsx:', error);
      }
    };
    if (serverAddress) fetchModelConfig();
  }, [serverAddress]);

  // Load device preferences on startup
  useEffect(() => {
    const loadDevicePreferences = async () => {
      try {
        const savedDevices = localStorage.getItem('device_preferences');
        if (savedDevices) {
          const prefs = JSON.parse(savedDevices);
          if (prefs && (prefs.micDevice || prefs.systemDevice)) {
            setSelectedDevices(prefs);
            console.log('Loaded device preferences from localStorage:', prefs);
          }
        }
      } catch (error) {
        console.log('No device preferences found or failed to load:', error);
      }
    };
    loadDevicePreferences();
  }, []);

  // Load language preference on startup
  useEffect(() => {
    const loadLanguagePreference = async () => {
      try {
        const savedLanguage = localStorage.getItem('language_preference');
        if (savedLanguage) {
          setSelectedLanguage(savedLanguage);
          console.log('Loaded language preference:', savedLanguage);
        } else {
          setSelectedLanguage('auto-translate');
        }
      } catch (error) {
        console.log('No language preference found or failed to load, using default (auto-translate):', error);
        setSelectedLanguage('auto-translate');
      }
    };
    loadLanguagePreference();
  }, []);

  const handleDiscardRecovery = async () => {
    if (confirm('Are you sure you want to discard this recovered meeting? This cannot be undone.')) {
      if (pendingRecoveryId) {
        await recoveryService.deletePendingTranscript(pendingRecoveryId);
        setPendingRecoveryId(null);
      }
      setTranscripts([]);
      setMeetingTitle('+ New Call');
      setCurrentMeeting(null);
      toast.info('Recovered meeting discarded');
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col h-screen bg-gray-50"
    >
      {showErrorAlert && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <Alert className="max-w-md mx-4 border-red-200 bg-white shadow-xl">
            <AlertTitle className="text-red-800">Recording Stopped</AlertTitle>
            <AlertDescription className="text-red-700">
              {errorMessage}
              <button
                onClick={() => setShowErrorAlert(false)}
                className="ml-2 text-red-600 hover:text-red-800 underline"
              >
                Dismiss
              </button>
            </AlertDescription>
          </Alert>
        </div>
      )}
      {showChunkDropWarning && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <Alert className="max-w-lg mx-4 border-yellow-200 bg-white shadow-xl">
            <AlertTitle className="text-yellow-800">Transcription Performance Warning</AlertTitle>
            <AlertDescription className="text-yellow-700">
              {chunkDropMessage}
              <button
                onClick={() => setShowChunkDropWarning(false)}
                className="ml-2 text-yellow-600 hover:text-yellow-800 underline"
              >
                Dismiss
              </button>
            </AlertDescription>
          </Alert>
        </div>
      )}
      <div className="flex flex-1 overflow-hidden">
        {/* Left side - Transcript */}
        <div ref={transcriptContainerRef} className="w-full border-r border-gray-200 bg-white flex flex-col overflow-y-auto">
          {/* Title area - Sticky header */}
          <div className="sticky top-0 z-10 bg-white p-4 border-gray-200">
            <div className="flex flex-col space-y-3">
              <div className="flex  flex-col space-y-2">
                <div className="flex justify-center  items-center space-x-2">
                  <ButtonGroup>
                    {transcripts?.length > 0 && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => {
                          handleCopyTranscript();
                        }}
                        title="Copy Transcript"
                      >
                        <Copy />
                        <span className='hidden md:inline'>
                          Copy
                        </span>
                      </Button>
                    )}
                    {/* {!isRecording && transcripts?.length === 0 && ( */}
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowModelSelector(true)}
                      title="Transcription Model Settings"
                    >
                      <Settings />
                      <span className='hidden md:inline'>
                        Model
                      </span>
                    </Button>

                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowDeviceSettings(true)}
                      title="Input/Output devices selection"
                    >
                      <MicrophoneIcon />
                      <span className='hidden md:inline'>
                        Devices
                      </span>
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowLanguageSettings(true)}
                      title="Language"
                    >
                      <GlobeIcon />
                      <span className='hidden md:inline'>
                        Language
                      </span>
                    </Button>
                  </ButtonGroup>
                  {/* {showSummary && !isRecording && (
                    <>
                      <button
                        onClick={handleGenerateSummary}
                        disabled={summaryStatus === 'processing'}
                        className={`px-3 py-2 border rounded-md transition-all duration-200 inline-flex items-center gap-2 shadow-sm ${
                          summaryStatus === 'processing'
                            ? 'bg-yellow-50 border-yellow-200 text-yellow-700'
                            : transcripts.length === 0
                            ? 'bg-gray-50 border-gray-200 text-gray-400 cursor-not-allowed'
                            : 'bg-green-50 border-green-200 text-green-700 hover:bg-green-100 hover:border-green-300 active:bg-green-200'
                        }`}
                        title={
                          summaryStatus === 'processing'
                            ? 'Generating summary...'
                            : transcripts.length === 0
                            ? 'No transcript available'
                            : 'Generate AI Summary'
                        }
                      >
                        {summaryStatus === 'processing' ? (
                          <>
                            <svg className="animate-spin h-4 w-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                            </svg>
                            <span className="text-sm">Processing...</span>
                          </>
                        ) : (
                          <>
                            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                              <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                            </svg>
                            <span className="text-sm">Generate Note</span>
                          </>
                        )}
                      </button>
                      <button
                        onClick={() => setShowModelSettings(true)}
                        className="px-3 py-2 border rounded-md transition-all duration-200 inline-flex items-center gap-2 shadow-sm bg-gray-50 border-gray-200 text-gray-700 hover:bg-gray-100 hover:border-gray-300 active:bg-gray-200"
                        title="Model Settings"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                          <path strokeLinecap="round" strokeLinejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                          <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                        </svg>
                      </button>
                    </>
                  )} */}
                </div>

                {/* {showSummary && !isRecording && (
                  <>
                    <button
                      onClick={handleGenerateSummary}
                      disabled={summaryStatus === 'processing'}
                      className={`px-3 py-2 border rounded-md transition-all duration-200 inline-flex items-center gap-2 shadow-sm ${
                        summaryStatus === 'processing'
                          ? 'bg-yellow-50 border-yellow-200 text-yellow-700'
                          : transcripts.length === 0
                          ? 'bg-gray-50 border-gray-200 text-gray-400 cursor-not-allowed'
                          : 'bg-green-50 border-green-200 text-green-700 hover:bg-green-100 hover:border-green-300 active:bg-green-200'
                      }`}
                      title={
                        summaryStatus === 'processing'
                          ? 'Generating summary...'
                          : transcripts.length === 0
                          ? 'No transcript available'
                          : 'Generate AI Summary'
                      }
                    >
                      {summaryStatus === 'processing' ? (
                        <>
                          <svg className="animate-spin h-4 w-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                          </svg>
                          <span className="text-sm">Processing...</span>
                        </>
                      ) : (
                        <>
                          <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                            <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                          </svg>
                          <span className="text-sm">Generate Note</span>
                        </>
                      )}
                    </button>
                    <button
                      onClick={() => setShowModelSettings(true)}
                      className="px-3 py-2 border rounded-md transition-all duration-200 inline-flex items-center gap-2 shadow-sm bg-gray-50 border-gray-200 text-gray-700 hover:bg-gray-100 hover:border-gray-300 active:bg-gray-200"
                      title="Model Settings"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path strokeLinecap="round" strokeLinejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                        <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                      </svg>
                    </button>
                  </>
                )} */}
              </div>
            </div>
          </div>

          {/* Permission Warning (only for Tauri mode) */}


          {/* Transcript content */}
          <div className="pb-20">
            <div className="flex justify-center">
              <div className="w-2/3 max-w-[750px]">
                <TranscriptView
                  transcripts={transcripts}
                  isRecording={isRecording}
                  isPaused={isPaused}
                  activeDuration={recordingElapsedSeconds}
                  isProcessing={isProcessingStop}
                  isStopping={isStopping}
                  enableStreaming={isRecording}
                />
              </div>
            </div>
          </div>

          {/* Custom prompt input at bottom of transcript section */}
          {/* {!isRecording && transcripts.length > 0 && !isMeetingActive && (
            <div className="p-4 border-t border-gray-200">
              <textarea
                placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
                className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
                value={customPrompt}
                onChange={(e) => setCustomPrompt(e.target.value)}
                disabled={summaryStatus === 'processing'}
              />
            </div>
          )} */}

          {/* Recording controls - only show when not in recovery mode OR when recording is active */}
          {(!isProcessingStop && !isSavingTranscript && (!pendingRecoveryId || isRecording)) && (
            <div className="fixed bottom-12 left-0 right-0 z-10">
              <div
                className="flex justify-center pl-8 transition-[margin] duration-300"
                style={{
                  marginLeft: sidebarCollapsed ? '4rem' : '16rem'
                }}
              >
                <div className="w-2/3 max-w-[750px] flex justify-center">
                  <div className="bg-white rounded-full shadow-lg flex items-center">
                    <RecordingControls
                      isRecording={isRecording}
                      onRecordingStop={(success) => handleRecordingStop(success)}
                      onRecordingStart={handleRecordingStart}
                      onTranscriptReceived={handleTranscriptUpdate}
                      onStopInitiated={() => setIsStopping(true)}
                      barHeights={barHeights}
                      onTranscriptionError={(message) => {
                        setErrorMessage(message);
                        setShowErrorAlert(true);
                      }}
                      isRecordingDisabled={isRecordingDisabled}
                      isParentProcessing={isProcessingStop}
                      selectedDevices={selectedDevices}
                      meetingName={meetingTitle}
                      onSessionIdReceived={(sessionId) => {
                        setCurrentSessionId(sessionId);
                        // Upgrade temporary recovery ids once backend confirms live session id.
                        setPendingRecoveryId((prev) => {
                          if (!prev) return prev;
                          if (prev === sessionId) return prev;
                          return prev.startsWith('recovery-') ? sessionId : prev;
                        });
                      }}
                      initialSessionId={currentSessionId}
                      onPauseChange={setIsPaused}
                      startSignal={resumeStartSignal}
                    />
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Recovery Mode Actions - hide if currently recording */}
          {pendingRecoveryId && !isSavingTranscript && !isRecording && (
            <div className="fixed bottom-12 left-0 right-0 z-10">
              <div
                className="flex justify-center pl-8 transition-[margin] duration-300"
                style={{
                  marginLeft: sidebarCollapsed ? '4rem' : '16rem'
                }}
              >
                <div className="bg-amber-50 border border-amber-200 rounded-full shadow-lg px-6 py-3 flex items-center gap-4">
                  <div className="flex flex-col">
                    <span className="text-sm font-semibold text-amber-800">Recovery Mode</span>
                    <span className="text-xs text-amber-600 max-w-[200px] truncate" title={meetingTitle}>
                      {meetingTitle}
                    </span>
                  </div>
                  <div className="h-8 w-px bg-amber-200 mx-2"></div>
                  <Button
                    onClick={() => {
                      // Reveal controls and trigger actual websocket/audio start.
                      setPendingRecoveryId(null);
                      setResumeStartSignal((v) => v + 1);
                    }}
                    className="bg-blue-600 hover:bg-blue-700 text-white rounded-full px-6"
                  >
                    Resume Meeting
                  </Button>
                  <Button
                    onClick={() => handleWebAudioRecordingStop()}
                    className="bg-green-600 hover:bg-green-700 text-white rounded-full px-6"
                  >
                    Save & Finish
                  </Button>
                  <Button
                    variant="outline"
                    onClick={handleDiscardRecovery}
                    className="border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 rounded-full px-4"
                  >
                    Discard
                  </Button>
                </div>
              </div>
            </div>
          )}

          {/* Processing status overlay */}
          {summaryStatus === 'processing' && !isRecording && (
            <div className="fixed bottom-4 left-0 right-0 z-10">
              <div
                className="flex justify-center pl-8 transition-[margin] duration-300"
                style={{
                  marginLeft: sidebarCollapsed ? '4rem' : '16rem'
                }}
              >
                <div className="w-2/3 max-w-[750px] flex justify-center">
                  <div className="bg-white rounded-lg shadow-lg px-4 py-2 flex items-center space-x-2">
                    <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-gray-900"></div>
                    <span className="text-sm text-gray-700">Finalizing transcription...</span>
                  </div>
                </div>
              </div>
            </div>
          )}
          {isSavingTranscript && (
            <div className="fixed bottom-4 left-0 right-0 z-10">
              <div
                className="flex justify-center pl-8 transition-[margin] duration-300"
                style={{
                  marginLeft: sidebarCollapsed ? '4rem' : '16rem'
                }}
              >
                <div className="w-2/3 max-w-[750px] flex justify-center">
                  <div className="bg-white rounded-lg shadow-lg px-4 py-2 flex items-center space-x-2">
                    <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-gray-900"></div>
                    <span className="text-sm text-gray-700">Saving transcript...</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Preferences Modal (Settings) */}
          {showModelSettings && (
            <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
              <div className="bg-white rounded-lg shadow-xl max-w-4xl w-full max-h-[90vh] overflow-hidden flex flex-col">
                {/* Header */}
                <div className="flex justify-between items-center p-6 border-b">
                  <h3 className="text-xl font-semibold text-gray-900">Preferences</h3>
                  <button
                    onClick={() => setShowModelSettings(false)}
                    className="text-gray-500 hover:text-gray-700"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>

                {/* Content - Scrollable */}
                <div className="flex-1 overflow-y-auto p-6 space-y-8">
                  {/* General Preferences Section */}
                  <PreferenceSettings />

                  {/* Divider */}
                  <div className="border-t pt-8">
                    <h4 className="text-lg font-semibold text-gray-900 mb-4">AI Model Configuration</h4>
                    <div className="space-y-4">
                      <div>
                        <label className="block text-sm font-medium text-gray-700 mb-1">
                          Summarization Model
                        </label>
                        <div className="flex space-x-2">
                          <select
                            className="px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                            value={modelConfig.provider}
                            onChange={(e) => {
                              const provider = e.target.value as ModelConfig['provider'];
                              setModelConfig({
                                ...modelConfig,
                                provider,
                                model: (modelOptions[provider] || modelOptions.ollama)[0]
                              });
                            }}
                          >
                            <option value="claude">Claude</option>
                            <option value="groq">Groq</option>
                            <option value="ollama">Ollama</option>
                            <option value="openrouter">OpenRouter</option>
                          </select>

                          <select
                            className="flex-1 px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
                            value={modelConfig.model}
                            onChange={(e) => setModelConfig(prev => ({ ...prev, model: e.target.value }))}
                          >
                            {(modelOptions[modelConfig.provider] || []).map((model: string) => (
                              <option key={model} value={model}>
                                {model}
                              </option>
                            ))}
                          </select>
                        </div>
                      </div>
                      {modelConfig.provider === 'ollama' && (
                        <div>
                          <h4 className="text-lg font-bold mb-4">Available Ollama Models</h4>
                          {error && (
                            <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4">
                              {error}
                            </div>
                          )}
                          <div className="grid gap-4 max-h-[400px] overflow-y-auto pr-2">
                            {models.map((model) => (
                              <div
                                key={model.id}
                                className={`bg-white p-4 rounded-lg shadow cursor-pointer transition-colors ${modelConfig.model === model.name ? 'ring-2 ring-blue-500 bg-blue-50' : 'hover:bg-gray-50'
                                  }`}
                                onClick={() => setModelConfig(prev => ({ ...prev, model: model.name }))}
                              >
                                <h3 className="font-bold">{model.name}</h3>
                                <p className="text-gray-600">Size: {model.size}</p>
                                <p className="text-gray-600">Modified: {model.modified}</p>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                </div>

                {/* Footer */}
                <div className="border-t p-6 flex justify-end">
                  <button
                    onClick={() => setShowModelSettings(false)}
                    className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500"
                  >
                    Done
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Device Settings Modal */}
          {showDeviceSettings && (
            <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
              <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                <div className="flex justify-between items-center mb-4">
                  <h3 className="text-lg font-semibold text-gray-900">Audio Device Settings</h3>
                  <button
                    onClick={() => setShowDeviceSettings(false)}
                    className="text-gray-500 hover:text-gray-700"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>

                <DeviceSelection
                  selectedDevices={selectedDevices}
                  onDeviceChange={setSelectedDevices}
                  disabled={isRecording}
                />

                <div className="mt-6 flex justify-end">
                  <button
                    onClick={() => {
                      const micDevice = selectedDevices.micDevice || 'Default';
                      const systemDevice = selectedDevices.systemDevice || 'Default';
                      toast.success("Devices selected", {
                        description: `Microphone: ${micDevice}, System Audio: ${systemDevice}`
                      });
                      setShowDeviceSettings(false);
                    }}
                    className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500"
                  >
                    Done
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Language Settings Modal */}
          {showLanguageSettings && (
            <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
              <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                <div className="flex justify-between items-center mb-4">
                  <h3 className="text-lg font-semibold text-gray-900">Language Settings</h3>
                  <button
                    onClick={() => setShowLanguageSettings(false)}
                    className="text-gray-500 hover:text-gray-700"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>

                <LanguageSelection
                  selectedLanguage={selectedLanguage}
                  onLanguageChange={setSelectedLanguage}
                  disabled={isRecording}
                  provider={transcriptModelConfig.provider}
                />

                <div className="mt-6 flex justify-end">
                  <button
                    onClick={() => setShowLanguageSettings(false)}
                    className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500"
                  >
                    Done
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Model Selection Modal - shown when model loading fails */}
          {showModelSelector && (
            <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
              <div className="bg-white rounded-lg max-w-4xl w-full mx-4 shadow-xl max-h-[90vh] flex flex-col">
                {/* Fixed Header */}
                <div className="flex justify-between items-center p-6 pb-4 border-b border-gray-200">
                  <h3 className="text-lg font-semibold text-gray-900">
                    {modelSelectorMessage ? 'Speech Recognition Setup Required' : 'Transcription Model Settings'}
                  </h3>
                  <button
                    onClick={() => {
                      setShowModelSelector(false);
                      setModelSelectorMessage(''); // Clear the message when closing
                    }}
                    className="text-gray-500 hover:text-gray-700"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>

                {/* Scrollable Content */}
                <div className="flex-1 overflow-y-auto p-6 pt-4">
                  {/* Only show warning if there's an error message (triggered by transcription error) */}
                  {modelSelectorMessage && (
                    <div className="mb-6 p-4 bg-yellow-50 border border-yellow-200 rounded-lg">
                      <div className="flex items-start space-x-3">
                        <span className="text-yellow-600 text-xl">⚠️</span>
                        <div>
                          <h4 className="font-medium text-yellow-800 mb-1">Model Required</h4>
                          <p className="text-sm text-yellow-700">
                            {modelSelectorMessage}
                          </p>
                        </div>
                      </div>
                    </div>
                  )}

                  <TranscriptSettings
                    transcriptModelConfig={transcriptModelConfig}
                    setTranscriptModelConfig={setTranscriptModelConfig}
                    onModelSelect={() => {
                      setShowModelSelector(false);
                      setModelSelectorMessage('');
                    }}
                  />
                </div>

                {/* Fixed Footer */}
                <div className="p-6 pt-4 border-t border-gray-200 flex items-center justify-between">
                  {/* Left side: Confidence Indicator Toggle */}
                  <div className="flex items-center gap-3">
                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={showConfidenceIndicator}
                        onChange={(e) => handleConfidenceToggle(e.target.checked)}
                        className="sr-only peer"
                      />
                      <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                    </label>
                    <div>
                      <p className="text-sm font-medium text-gray-700">Show Confidence Indicators</p>
                      <p className="text-xs text-gray-500">Display colored dots showing transcription confidence quality</p>
                    </div>
                  </div>

                  {/* Right side: Done Button */}
                  <button
                    onClick={() => {
                      setShowModelSelector(false);
                      setModelSelectorMessage(''); // Clear the message when closing
                    }}
                    className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 rounded-md hover:bg-gray-200 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-gray-500"
                  >
                    {modelSelectorMessage ? 'Cancel' : 'Done'}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Right side - AI Summary */}
        {/* <div className="flex-1 overflow-y-auto bg-white"> */}
        {/*   <div className="p-4 border-b border-gray-200"> */}
        {/*     <div className="flex items-center"> */}
        {/*       <EditableTitle */}
        {/*         title={meetingTitle} */}
        {/*         isEditing={isEditingTitle} */}
        {/*         onStartEditing={() => setIsEditingTitle(true)} */}
        {/*         onFinishEditing={() => setIsEditingTitle(false)} */}
        {/*         onChange={handleTitleChange} */}
        {/*       /> */}
        {/*     </div> */}
        {/*   </div> */}
        {/*   {/* {isSummaryLoading ? ( */}
        {/*     <div className="flex items-center justify-center h-full"> */}
        {/*       <div className="text-center"> */}
        {/*         <div className="inline-block animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-blue-500 mb-4"></div> */}
        {/*         <p className="text-gray-600">Generating AI Summary...</p> */}
        {/*       </div> */}
        {/*     </div> */}
        {/*   ) : showSummary && ( */}
        {/*     <div className="max-w-4xl mx-auto p-6"> */}
        {/*       {summaryResponse && ( */}
        {/*         <div className="fixed bottom-0 left-0 right-0 bg-white shadow-lg p-4 max-h-1/3 overflow-y-auto"> */}
        {/*           <h3 className="text-lg font-semibold mb-2">Meeting Summary</h3> */}
        {/*           <div className="grid grid-cols-2 gap-4"> */}
        {/*             <div className="bg-white p-4 rounded-lg shadow-sm"> */}
        {/*               <h4 className="font-medium mb-1">Key Points</h4> */}
        {/*               <ul className="list-disc pl-4"> */}
        {/*                 {summaryResponse.summary.key_points.blocks.map((block, i) => ( */}
        {/*                   <li key={i} className="text-sm">{block.content}</li> */}
        {/*                 ))} */}
        {/*               </ul> */}
        {/*             </div> */}
        {/*             <div className="bg-white p-4 rounded-lg shadow-sm mt-4"> */}
        {/*               <h4 className="font-medium mb-1">Action Items</h4> */}
        {/*               <ul className="list-disc pl-4"> */}
        {/*                 {summaryResponse.summary.action_items.blocks.map((block, i) => ( */}
        {/*                   <li key={i} className="text-sm">{block.content}</li> */}
        {/*                 ))} */}
        {/*               </ul> */}
        {/*             </div> */}
        {/*             <div className="bg-white p-4 rounded-lg shadow-sm mt-4"> */}
        {/*               <h4 className="font-medium mb-1">Decisions</h4> */}
        {/*               <ul className="list-disc pl-4"> */}
        {/*                 {summaryResponse.summary.decisions.blocks.map((block, i) => ( */}
        {/*                   <li key={i} className="text-sm">{block.content}</li> */}
        {/*                 ))} */}
        {/*               </ul> */}
        {/*             </div> */}
        {/*             <div className="bg-white p-4 rounded-lg shadow-sm mt-4"> */}
        {/*               <h4 className="font-medium mb-1">Main Topics</h4> */}
        {/*               <ul className="list-disc pl-4"> */}
        {/*                 {summaryResponse.summary.main_topics.blocks.map((block, i) => ( */}
        {/*                   <li key={i} className="text-sm">{block.content}</li> */}
        {/*                 ))} */}
        {/*               </ul> */}
        {/*             </div> */}
        {/*           </div> */}
        {/*           {summaryResponse.raw_summary ? ( */}
        {/*             <div className="mt-4"> */}
        {/*               <h4 className="font-medium mb-1">Full Summary</h4> */}
        {/*               <p className="text-sm whitespace-pre-wrap">{summaryResponse.raw_summary}</p> */}
        {/*             </div> */}
        {/*           ) : null} */}
        {/*         </div> */}
        {/*       )} */}
        {/*       <div className="flex-1 overflow-y-auto p-4"> */}
        {/*         <AISummary  */}
        {/*           summary={aiSummary}  */}
        {/*           status={summaryStatus}  */}
        {/*           error={summaryError} */}
        {/*           onSummaryChange={(newSummary) => setAiSummary(newSummary)} */}
        {/*           onRegenerateSummary={handleRegenerateSummary} */}
        {/*         /> */}
        {/*       </div> */}
        {/*       {summaryStatus !== 'idle' && ( */}
        {/*         <div className={`mt-4 p-4 rounded-lg ${ */}
        {/*           summaryStatus === 'error' ? 'bg-red-100 text-red-700' : */}
        {/*           summaryStatus === 'completed' ? 'bg-green-100 text-green-700' : */}
        {/*           'bg-blue-100 text-blue-700' */}
        {/*         }`}> */}
        {/*           <p className="text-sm font-medium">{getSummaryStatusMessage(summaryStatus)}</p> */}
        {/*         </div> */}
        {/*       )} */}
        {/*     </div> */}
        {/*   )} */}        {/* </div> */}
      </div>

      {/* Chat Interface - Only show when recording or meeting is active */}
      {(isRecording || isMeetingActive) && (
        <>
          {isChatOpen && (
            <ChatInterface
              meetingId={'current-recording'}
              currentTranscripts={transcripts}
              onClose={() => setIsChatOpen(false)}
            />
          )}

          {!isChatOpen && (
            <div className="fixed bottom-6 right-6 flex flex-col gap-3 z-40">
              {/* Catch Me Up Button with Time Menu */}
              <div className="relative">
                <motion.button
                  initial={{ scale: 0 }}
                  animate={{ scale: 1 }}
                  whileHover={{ scale: 1.05 }}
                  whileTap={{ scale: 0.95 }}
                  onClick={() => setShowCatchUpMenu(!showCatchUpMenu)}
                  className="p-3 bg-amber-500 text-white rounded-full shadow-lg hover:bg-amber-600 transition-colors flex items-center gap-2"
                  title="Get a quick summary of the meeting so far"
                >
                  <Zap className="w-5 h-5" />
                  <span className="font-medium text-sm">Catch Up</span>
                </motion.button>

                {/* Time Selection Menu */}
                {showCatchUpMenu && (
                  <motion.div
                    initial={{ opacity: 0, scale: 0.9, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    className="absolute bottom-full right-0 mb-2 bg-white rounded-lg shadow-xl border border-gray-200 overflow-hidden min-w-[180px]"
                  >
                    <div className="p-2 border-b border-gray-100">
                      <p className="text-xs text-gray-500 font-medium px-2">Summarize:</p>
                    </div>
                    <div className="p-1">
                      {[
                        { label: 'Last 5 mins', value: 5 },
                        { label: 'Last 10 mins', value: 10 },
                        { label: 'Last 30 mins', value: 30 },
                        { label: 'Entire meeting', value: null },
                      ].map((option) => {
                        const meetingDuration = getMeetingDurationMinutes();
                        const isDisabled = option.value !== null && option.value > meetingDuration && option.value > 2;
                        const isMinNotMet = meetingDuration < 0.5; // Less than 30 seconds

                        return (
                          <button
                            key={option.label}
                            onClick={() => {
                              if (!isDisabled && !isMinNotMet) {
                                handleCatchUp(option.value);
                              }
                            }}
                            disabled={isDisabled || isMinNotMet}
                            className={`w-full text-left px-3 py-2 text-sm rounded transition-colors ${isDisabled || isMinNotMet
                              ? 'text-gray-300 cursor-not-allowed'
                              : 'text-gray-700 hover:bg-amber-50 hover:text-amber-700'
                              }`}
                          >
                            {option.label}
                            {option.value !== null && meetingDuration > 0 && option.value > meetingDuration && (
                              <span className="text-xs text-gray-400 ml-1">(not enough)</span>
                            )}
                          </button>
                        );
                      })}
                    </div>
                    <div className="p-2 border-t border-gray-100">
                      <div className="flex items-center gap-2">
                        <input
                          type="number"
                          min="2"
                          max={Math.max(2, getMeetingDurationMinutes())}
                          placeholder="Custom"
                          value={customMinutesInput}
                          onChange={(e) => setCustomMinutesInput(e.target.value)}
                          className="w-20 px-2 py-1 text-sm border border-gray-200 rounded focus:outline-none focus:ring-1 focus:ring-amber-500"
                        />
                        <span className="text-xs text-gray-500">mins</span>
                        <button
                          onClick={() => {
                            const mins = parseInt(customMinutesInput);
                            if (mins >= 2 && mins <= getMeetingDurationMinutes()) {
                              handleCatchUp(mins);
                              setCustomMinutesInput('');
                            } else {
                              toast.error(`Enter 2-${getMeetingDurationMinutes()} minutes`);
                            }
                          }}
                          className="px-2 py-1 bg-amber-500 text-white text-xs rounded hover:bg-amber-600"
                        >
                          Go
                        </button>
                      </div>
                    </div>
                  </motion.div>
                )}
              </div>

              {/* Ask AI Button */}
              <motion.button
                initial={{ scale: 0 }}
                animate={{ scale: 1 }}
                whileHover={{ scale: 1.05 }}
                whileTap={{ scale: 0.95 }}
                onClick={() => setIsChatOpen(true)}
                className="p-4 bg-blue-600 text-white rounded-full shadow-lg hover:bg-blue-700 transition-colors flex items-center gap-2"
                title="Ask AI about this active meeting"
              >
                <Bot className="w-6 h-6" />
                <span className="font-medium">Ask AI</span>
              </motion.button>
            </div>
          )}

          {/* Catch Me Up Modal */}
          {isCatchUpOpen && (
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 20 }}
              className="fixed bottom-6 right-6 w-80 bg-white rounded-lg shadow-2xl border border-gray-200 z-50 overflow-hidden"
            >
              <div className="bg-amber-500 text-white px-4 py-3 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Zap className="w-5 h-5" />
                  <div>
                    <span className="font-semibold">Catch Me Up</span>
                    {catchUpMinutes !== null && (
                      <span className="text-xs opacity-80 ml-1">(Last {catchUpMinutes} mins)</span>
                    )}
                  </div>
                </div>
                <button
                  onClick={() => setIsCatchUpOpen(false)}
                  className="text-white hover:text-amber-100 transition-colors"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>
              <div className="p-4 max-h-80 overflow-y-auto">
                {isCatchUpLoading && !catchUpSummary ? (
                  <div className="flex items-center gap-2 text-gray-500">
                    <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-amber-500"></div>
                    <span className="text-sm">Generating summary...</span>
                  </div>
                ) : catchUpSummary ? (
                  <div className="text-sm text-gray-700 whitespace-pre-wrap leading-relaxed">
                    {catchUpSummary}
                    {isCatchUpLoading && (
                      <span className="inline-block w-2 h-4 bg-amber-500 animate-pulse ml-1"></span>
                    )}
                  </div>
                ) : (
                  <p className="text-sm text-gray-500">No transcript available yet.</p>
                )}
              </div>
            </motion.div>
          )}
        </>
      )}

      {/* Re-auth Modal */}
      {showReauthModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
            <h3 className="text-lg font-semibold text-gray-900 mb-2">Session Expired</h3>
            <p className="text-gray-600 mb-6">
              Your session has expired, but your meeting transcript is safe locally.
              Please log in again in a new tab, then click "Retry Save".
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => window.open('/login', '_blank')}
                className="px-4 py-2 text-sm font-medium text-blue-600 hover:bg-blue-50 rounded-md"
              >
                Log In (New Tab)
              </button>
              <button
                onClick={async () => {
                  setShowReauthModal(false);
                  await handleWebAudioRecordingStop();
                }}
                className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700"
              >
                Retry Save
              </button>
            </div>
          </div>
        </div>
      )}

    </motion.div>
  );
}

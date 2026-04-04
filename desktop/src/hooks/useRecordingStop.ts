import { useCallback, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import Analytics from '@/lib/analytics';

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

export interface RecordingStoppedPayload {
  meeting_id: string;
  meeting_title: string;
  folder_path?: string;
  transcript_count: number;
  duration_seconds: number;
  source_type: string;
  transcription_timed_out: boolean;
  save_error?: string;
  finalized_at: string;
}

interface UseRecordingStopReturn {
  handleRecordingStop: (callApi: boolean, payload?: RecordingStoppedPayload) => Promise<void>;
  isStopping: boolean;
  isProcessingTranscript: boolean;
  isSavingTranscript: boolean;
  summaryStatus: SummaryStatus;
  setIsStopping: (value: boolean) => void;
}

export function useRecordingStop(
  setIsRecording: (value: boolean) => void,
  setIsRecordingDisabled: (value: boolean) => void
): UseRecordingStopReturn {
  const recordingState = useRecordingState();
  const {
    status,
    setStatus,
    isStopping,
    isProcessing: isProcessingTranscript,
    isSaving: isSavingTranscript
  } = recordingState;

  const {
    transcriptsRef,
    flushBuffer,
    clearTranscripts,
    meetingTitle,
  } = useTranscripts();

  const {
    refetchMeetings,
    setCurrentMeeting,
    setIsMeetingActive,
  } = useSidebar();

  const router = useRouter();
  const stopInProgressRef = useRef(false);
  const lastProcessedFinalizationKeyRef = useRef<string | null>(null);

  const handleRecordingStop = useCallback(async (isCallApi: boolean, payload?: RecordingStoppedPayload) => {
    if (stopInProgressRef.current) {
      return;
    }

    stopInProgressRef.current = true;
    try {
      if (!isCallApi && !payload) {
        throw new Error('Missing backend finalization result');
      }
      if (!payload) {
        throw new Error('Stop command completed without finalization data');
      }

      const dedupeKey = `${payload.meeting_id}:${payload.finalized_at}`;
      const globalScope = window as Window & { __meetfreeLastFinalizationKey?: string };
      if (
        lastProcessedFinalizationKeyRef.current === dedupeKey ||
        globalScope.__meetfreeLastFinalizationKey === dedupeKey
      ) {
        return;
      }
      lastProcessedFinalizationKeyRef.current = dedupeKey;
      globalScope.__meetfreeLastFinalizationKey = dedupeKey;

      setStatus(RecordingStatus.STOPPING, 'Stopping recording...');
      setIsRecording(false);
      setIsRecordingDisabled(true);

      setStatus(RecordingStatus.PROCESSING_TRANSCRIPTS, 'Finalizing recording...');
      flushBuffer();
      await new Promise((resolve) => setTimeout(resolve, 250));

      if (!payload.meeting_id) {
        throw new Error(payload.save_error || 'Meeting finalization returned no meeting ID');
      }

      const meetingId = payload.meeting_id;
      const meetingName = payload.meeting_title || meetingTitle || 'New Meeting';
      const transcriptCount = payload.transcript_count ?? transcriptsRef.current.length;

      setStatus(RecordingStatus.SAVING, 'Refreshing meeting library...');
      await refetchMeetings();
      setCurrentMeeting({ id: meetingId, title: meetingName });
      setStatus(RecordingStatus.COMPLETED);

      toast.success('Recording saved successfully!', {
        description: payload.transcription_timed_out
          ? `${transcriptCount} transcript segments saved. Transcription hit the shutdown timeout, so some late segments may be missing.`
          : `${transcriptCount} transcript segments saved.`,
        action: {
          label: 'View Meeting',
          onClick: () => {
            router.push(`/meeting-details?id=${meetingId}`);
            Analytics.trackButtonClick('view_meeting_from_toast', 'recording_complete');
          }
        },
        duration: 10000,
      });

      try {
        const freshTranscripts = [...transcriptsRef.current];
        const durationSeconds = payload.duration_seconds || 0;
        const transcriptWordCount = freshTranscripts
          .map(t => t.text.split(/\s+/).length)
          .reduce((a, b) => a + b, 0);

        const wordsPerMinute = durationSeconds > 0 ? transcriptWordCount / (durationSeconds / 60) : 0;
        const meetingsToday = await Analytics.getMeetingsCountToday();

        await Analytics.trackMeetingCompleted(meetingId, {
          duration_seconds: durationSeconds,
          transcript_segments: transcriptCount,
          transcript_word_count: transcriptWordCount,
          words_per_minute: wordsPerMinute,
          meetings_today: meetingsToday
        });

        await Analytics.updateMeetingCount();

        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('analytics.json');
        const totalMeetings = await store.get<number>('total_meetings');

        if (totalMeetings === 1) {
          const daysSinceInstall = await Analytics.calculateDaysSince('first_launch_date');
          await Analytics.track('user_activated', {
            meetings_count: '1',
            days_since_install: daysSinceInstall?.toString() || 'null',
            first_meeting_duration_seconds: durationSeconds.toString()
          });
        }
      } catch (analyticsError) {
        console.error('Failed to track meeting completion analytics:', analyticsError);
      }

      setIsMeetingActive(false);
      setIsRecordingDisabled(false);

      setTimeout(() => {
        router.push(`/meeting-details?id=${meetingId}&source=recording`);
        clearTranscripts();
        Analytics.trackPageView('meeting_details');
        setStatus(RecordingStatus.IDLE);
      }, 1200);
    } catch (error) {
      console.error('Error in handleRecordingStop:', error);
      if (isCallApi) {
        setIsMeetingActive(false);
      }
      setStatus(RecordingStatus.ERROR, error instanceof Error ? error.message : 'Unknown error');
      setIsRecordingDisabled(false);

      if (isCallApi) {
        toast.error('Failed to save meeting', {
          description: error instanceof Error ? error.message : 'Unknown error'
        });
      }
    } finally {
      stopInProgressRef.current = false;
    }
  }, [
    setStatus,
    setIsRecording,
    setIsRecordingDisabled,
    flushBuffer,
    meetingTitle,
    transcriptsRef,
    refetchMeetings,
    setCurrentMeeting,
    setIsMeetingActive,
    router,
    clearTranscripts,
  ]);

  const summaryStatus: SummaryStatus = status === RecordingStatus.PROCESSING_TRANSCRIPTS ? 'processing' : 'idle';

  return {
    handleRecordingStop,
    isStopping,
    isProcessingTranscript,
    isSavingTranscript,
    summaryStatus,
    setIsStopping: (value: boolean) => {
      setStatus(value ? RecordingStatus.STOPPING : RecordingStatus.IDLE);
    },
  };
}

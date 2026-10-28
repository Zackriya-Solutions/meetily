import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import Analytics from '@/lib/analytics';
import type { RecordingCompletion } from '@/types';
import type { RecordingState as NativeState } from '@/services/recordingService';

// Native events and the Stop response share presentation work, but an explicit
// Stop must still navigate if a native completion was already presented.
const presentations = new Map<string, Promise<void>>();
const navigatedSessions = new Set<string>();

async function canPresentSession(sessionId: string): Promise<boolean> {
  try {
    const live = await invoke<NativeState>('get_recording_state');
    return !live.session || live.session.sessionId === sessionId
      || !['starting', 'recording', 'paused', 'stopping', 'processing', 'saving'].includes(live.session.phase);
  } catch (error) {
    console.warn('Could not confirm current recording owner:', error);
    return false;
  }
}

export function useRecordingStop(
  setIsRecording: (value: boolean) => void,
  setIsRecordingDisabled: (value: boolean) => void,
) {
  const recordingState = useRecordingState();
  const { clearTranscripts, markMeetingAsSaved } = useTranscripts();
  const { refetchMeetings, setCurrentMeeting, setIsMeetingActive } = useSidebar();
  const router = useRouter();

  const handleRecordingCompletion = useCallback(async (
    result: RecordingCompletion,
    navigate = true,
  ) => {
    if (!result.meetingId) return;
    const viewMeeting = () => router.push(`/meeting-details?id=${result.meetingId}&source=recording`);
    let presentation = presentations.get(result.sessionId);
    if (!presentation) {
      presentation = (async () => {
        // Keep failed sessions recoverable. Browser cleanup and history refresh
        // must not hold up opening a meeting already committed by Rust.
        if (!result.error) {
          void markMeetingAsSaved(result.sessionId)
            .catch(error => console.warn('Browser recovery cleanup failed:', error));
        }
        void refetchMeetings().catch(error => {
          console.warn('Meeting saved natively; history refresh failed:', error);
          toast.warning('Meeting saved; history could not refresh', { description: 'Reopen the meeting list to retry.' });
        });
        if (await canPresentSession(result.sessionId)) {
          setIsRecording(false); setIsRecordingDisabled(false); setIsMeetingActive(false);
          setCurrentMeeting({ id: result.meetingId, title: result.meetingTitle });
          recordingState.setStatus(result.error ? RecordingStatus.ERROR : RecordingStatus.COMPLETED, result.error || undefined);
          clearTranscripts();
        }
        if (result.error) {
          toast.warning('Meeting saved with a processing error', {
            description: result.error, action: { label: 'View Meeting', onClick: viewMeeting }, duration: 15000,
          });
        } else {
          toast.success('Recording saved successfully!', {
            description: `${result.transcriptCount} transcript segments saved.`,
            action: { label: 'View Meeting', onClick: viewMeeting }, duration: 10000,
          });
        }
        void Analytics.updateMeetingCount().catch(error => console.warn('Meeting analytics update failed:', error));
      })();
      presentations.set(result.sessionId, presentation);
    }
    await presentation;
    // Recheck ownership after presentation: another session may have started.
    if (navigate && !navigatedSessions.has(result.sessionId) && await canPresentSession(result.sessionId)) {
      if (!navigatedSessions.has(result.sessionId)) {
        navigatedSessions.add(result.sessionId);
        viewMeeting();
      }
    }
  }, [setIsRecording, setIsRecordingDisabled, setIsMeetingActive, markMeetingAsSaved, refetchMeetings,
    setCurrentMeeting, recordingState.setStatus, router, clearTranscripts]);

  const handleRecordingStop = useCallback(async (callApi: boolean, completion?: RecordingCompletion | null) => {
    if (!callApi) return;
    // Rust finishes transcription and persistence before stop resolves. Never
    // reconstruct or resave the meeting from React transcript state.
    try {
      const result = completion === undefined ? await invoke<RecordingCompletion | null>('get_last_recording_result') : completion;
      if (result) await handleRecordingCompletion(result, true);
    } catch (error) { toast.error('Could not refresh the saved meeting', { description: String(error) }); }
  }, [handleRecordingCompletion]);

  return {
    handleRecordingStop, handleRecordingCompletion,
    isStopping: recordingState.isStopping,
    isProcessingTranscript: recordingState.isProcessing,
    isSavingTranscript: recordingState.isSaving,
    summaryStatus: recordingState.isProcessing ? 'processing' as const : 'idle' as const,
    setIsStopping: (value: boolean) => recordingState.setStatus(value ? RecordingStatus.STOPPING : RecordingStatus.IDLE),
  };
}

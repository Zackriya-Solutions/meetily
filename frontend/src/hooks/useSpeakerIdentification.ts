import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { SpeakerJobStatus } from '@/types';
import { formatSpeakerCount } from '@/lib/speakers';

interface CompletePayload { meeting_id: string; speaker_count: number; automatic: boolean; warning?: string | null }
interface ErrorPayload { meeting_id: string; error: string; automatic: boolean; cancelled?: boolean }

export function useSpeakerIdentification(meetingId: string | null, onComplete: () => void | Promise<void>) {
  const [job, setJob] = useState<SpeakerJobStatus | null>(null);
  const [statusKnown, setStatusKnown] = useState(false);
  const onCompleteRef = useRef(onComplete);
  onCompleteRef.current = onComplete;

  // Listen first, then read the status; a status reply is ignored once any event has arrived,
  // so a job that finishes around mount cannot leave a stale 'running' state behind.
  useEffect(() => {
    setJob(null);
    setStatusKnown(false);
    if (!meetingId) return;
    let alive = true;
    let eventSeen = false;
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      const fns = await Promise.all([
        listen<SpeakerJobStatus>('diarization-progress', ({ payload }) => {
          if (payload.meeting_id !== meetingId) return;
          eventSeen = true;
          setJob(payload);
        }),
        listen<CompletePayload>('diarization-complete', async ({ payload }) => {
          if (payload.meeting_id !== meetingId) return;
          eventSeen = true;
          setJob(null);
          await onCompleteRef.current();
          if (payload.automatic) return;
          const identified = `Identified ${formatSpeakerCount(payload.speaker_count)}`;
          if (payload.warning) {
            toast.warning(identified, { description: payload.warning });
          } else {
            toast.success(identified);
          }
        }),
        listen<ErrorPayload>('diarization-error', ({ payload }) => {
          if (payload.meeting_id !== meetingId) return;
          eventSeen = true;
          setJob(null);
          if (payload.automatic) return;
          if (payload.cancelled) {
            toast.info('Speaker identification cancelled');
          } else {
            toast.error(payload.error);
          }
        }),
      ]);
      if (!alive) {
        fns.forEach((u) => u());
        return;
      }
      unlisteners.push(...fns);
      try {
        const status = await invoke<SpeakerJobStatus | null>('get_speaker_identification_status', { meetingId });
        if (alive && !eventSeen) setJob(status);
      } catch (error) {
        console.error('Failed to read speaker identification status:', error);
      } finally {
        if (alive) setStatusKnown(true);
      }
    })();
    return () => {
      alive = false;
      unlisteners.forEach((u) => u());
    };
  }, [meetingId]);

  // Events drive `job`: the backend emits the queued status before this invoke resolves, so
  // writing state here could overwrite a newer event (for example an immediate failure).
  const start = useCallback(async (folderPath: string, numSpeakers: number | null) => {
    if (!meetingId) return;
    await invoke('start_speaker_identification', { meetingId, meetingFolderPath: folderPath, numSpeakers });
  }, [meetingId]);

  const cancel = useCallback(async () => {
    if (!meetingId) return;
    try {
      await invoke('cancel_speaker_identification', { meetingId });
    } catch (error) {
      console.error('Failed to cancel speaker identification:', error);
      // The backend has no such job: resync so a stale banner clears.
      try {
        setJob(await invoke<SpeakerJobStatus | null>('get_speaker_identification_status', { meetingId }));
      } catch (statusError) {
        console.error('Failed to read speaker identification status:', statusError);
      }
    }
  }, [meetingId]);

  return { job, isActive: job !== null, statusKnown, start, cancel };
}

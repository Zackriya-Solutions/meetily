import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { MeetingSpeaker, NameOutcome, PropagatedLink } from '@/types';
import { buildSpeakerNameMap, formatPropagationMessage, propagatedMeetingCount } from '@/lib/speakers';

async function undoPropagation(links: PropagatedLink[]) {
  try {
    await invoke<string[]>('api_undo_name_propagation', { links });
  } catch (error) {
    console.error('Failed to undo name propagation:', error);
    toast.error('Failed to undo');
  }
}

export function useMeetingSpeakers(meetingId: string | null) {
  const [speakers, setSpeakers] = useState<MeetingSpeaker[]>([]);

  /** Reloads and returns the speakers, so a caller can decide on the fresh list. */
  const refetch = useCallback(async (): Promise<MeetingSpeaker[]> => {
    if (!meetingId) {
      setSpeakers([]);
      return [];
    }
    try {
      const next = await invoke<MeetingSpeaker[]>('api_list_meeting_speakers', { meetingId });
      setSpeakers(next);
      return next;
    } catch (error) {
      console.error('Failed to load meeting speakers:', error);
      return [];
    }
  }, [meetingId]);

  useEffect(() => {
    void refetch();
  }, [refetch]);

  const names = useMemo(() => buildSpeakerNameMap(speakers), [speakers]);

  // Naming a voice can name the same voice in other meetings; offer to take that back.
  const announce = useCallback((outcome: NameOutcome) => {
    const count = propagatedMeetingCount(outcome.propagated, meetingId ?? '');
    if (count === 0) return;
    const links = outcome.propagated;
    toast.success(formatPropagationMessage(count), {
      action: { label: 'Undo', onClick: () => { void undoPropagation(links); } },
      duration: 10000,
    });
  }, [meetingId]);

  const name = useCallback(async (speakerKey: string, name: string) => {
    const outcome = await invoke<NameOutcome>('api_name_meeting_speaker', { meetingId, speakerKey, name });
    await refetch();
    announce(outcome);
    return outcome;
  }, [meetingId, refetch, announce]);

  const confirm = useCallback(async (speakerKey: string) => {
    const outcome = await invoke<NameOutcome>('api_confirm_meeting_speaker_name', { meetingId, speakerKey });
    await refetch();
    announce(outcome);
    return outcome;
  }, [meetingId, refetch, announce]);

  const reject = useCallback(async (speakerKey: string) => {
    await invoke('api_reject_meeting_speaker_name', { meetingId, speakerKey });
    await refetch();
  }, [meetingId, refetch]);

  const merge = useCallback(async (fromKey: string, intoKey: string) => {
    await invoke('api_merge_meeting_speakers', { meetingId, fromKey, intoKey });
    await refetch();
  }, [meetingId, refetch]);

  const reassign = useCallback(async (transcriptId: string, speakerKey: string | null) => {
    const key = await invoke<string>('api_set_transcript_speaker', { meetingId, transcriptId, speakerKey });
    await refetch();
    return key;
  }, [meetingId, refetch]);

  return { speakers, names, refetch, name, confirm, reject, merge, reassign };
}

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MeetingSpeaker } from '@/types';
import { buildSpeakerNameMap } from '@/lib/speakers';

export function useMeetingSpeakers(meetingId: string | null) {
  const [speakers, setSpeakers] = useState<MeetingSpeaker[]>([]);

  const refetch = useCallback(async () => {
    if (!meetingId) {
      setSpeakers([]);
      return;
    }
    try {
      setSpeakers(await invoke<MeetingSpeaker[]>('api_list_meeting_speakers', { meetingId }));
    } catch (error) {
      console.error('Failed to load meeting speakers:', error);
    }
  }, [meetingId]);

  useEffect(() => {
    void refetch();
  }, [refetch]);

  const names = useMemo(() => buildSpeakerNameMap(speakers), [speakers]);

  const rename = useCallback(async (speakerKey: string, name: string) => {
    await invoke('api_name_meeting_speaker', { meetingId, speakerKey, name });
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

  return { speakers, names, refetch, rename, merge, reassign };
}

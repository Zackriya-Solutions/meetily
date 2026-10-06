import { invoke } from '@tauri-apps/api/core';
import type { PaginatedTranscriptsResponse, Transcript } from '@/types';

/**
 * Every row of a meeting, not only the loaded pages: one request for the count, one for the rows.
 * Errors reach the caller, which decides what to tell the user.
 */
export async function fetchAllMeetingTranscripts(meetingId: string): Promise<Transcript[]> {
  const first = await invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', { meetingId, limit: 1, offset: 0 });
  if (first.total_count === 0) return [];
  const all = await invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', {
    meetingId, limit: first.total_count, offset: 0,
  });
  return all.transcripts;
}

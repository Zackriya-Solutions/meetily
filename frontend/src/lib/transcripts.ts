import { invoke } from '@tauri-apps/api/core';
import type { PaginatedTranscriptsResponse, Transcript } from '@/types';

/** A negative limit is no limit in SQLite, which the command passes it to. */
const ALL_ROWS = -1;

/**
 * Every row of a meeting, not only the loaded pages. Errors reach the caller, which decides what
 * to tell the user.
 */
export async function fetchAllMeetingTranscripts(meetingId: string): Promise<Transcript[]> {
  const all = await invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', { meetingId, limit: ALL_ROWS, offset: 0 });
  return all.transcripts;
}

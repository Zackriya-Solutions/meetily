import { afterAll, beforeEach, describe, expect, mock, test } from 'bun:test';
import type { PaginatedTranscriptsResponse, Transcript } from '../../src/types';

const originalCore = { ...await import('@tauri-apps/api/core') };
const rows: Transcript[] = [
  { id: 'a', text: 'hi', timestamp: '10:00:00' },
  { id: 'b', text: 'there', timestamp: '10:00:02' },
];
const requests: Record<string, unknown>[] = [];
let total = rows.length;
const invoke = mock(async (command: string, args: Record<string, unknown>): Promise<PaginatedTranscriptsResponse> => {
  if (command !== 'api_get_meeting_transcripts') throw new Error(`unexpected command ${command}`);
  requests.push(args);
  // SQLite: a negative LIMIT has no upper bound.
  const limit = (args.limit as number) < 0 ? total : args.limit as number;
  return { transcripts: rows.slice(0, Math.min(limit, total)), total_count: total, has_more: limit < total };
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
const { fetchAllMeetingTranscripts } = await import('../../src/lib/transcripts');
afterAll(() => mock.module('@tauri-apps/api/core', () => originalCore));

beforeEach(() => {
  requests.length = 0;
  total = rows.length;
  mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
});

describe('fetchAllMeetingTranscripts', () => {
  test('reads every row in one request', async () => {
    expect(await fetchAllMeetingTranscripts('m1')).toEqual(rows);
    expect(requests).toEqual([{ meetingId: 'm1', limit: -1, offset: 0 }]);
  });

  test('an empty meeting has no rows', async () => {
    total = 0;
    expect(await fetchAllMeetingTranscripts('m1')).toEqual([]);
  });

  test('a failed request reaches the caller', async () => {
    invoke.mockImplementationOnce(async () => { throw 'Meeting not found'; });
    await expect(fetchAllMeetingTranscripts('m1')).rejects.toBe('Meeting not found');
  });
});

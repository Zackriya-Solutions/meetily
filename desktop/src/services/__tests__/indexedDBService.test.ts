import 'fake-indexeddb/auto';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IndexedDBService } from '@/services/indexedDBService';

const DB_NAME = 'MeetFreeRecoveryDB';

async function deleteDatabase(): Promise<void> {
  await new Promise((resolve) => {
    const request = indexedDB.deleteDatabase(DB_NAME);
    request.onsuccess = () => resolve(undefined);
    request.onerror = () => resolve(undefined);
    request.onblocked = () => resolve(undefined);
  });
}

describe('IndexedDBService', () => {
  beforeEach(async () => {
    await deleteDatabase();
    vi.restoreAllMocks();
  });

  it('retries initialization after an open failure', async () => {
    const originalIndexedDb = globalThis.indexedDB;
    const fakeDatabase = { close: vi.fn(), onversionchange: null } as unknown as IDBDatabase;
    let attempts = 0;

    globalThis.indexedDB = {
      open: vi.fn(() => {
        const request = {} as IDBOpenDBRequest;

        queueMicrotask(() => {
          attempts += 1;
          if (attempts === 1) {
            Object.defineProperty(request, 'error', {
              configurable: true,
              value: new Error('open failed'),
            });
            request.onerror?.(new Event('error'));
            return;
          }

          Object.defineProperty(request, 'result', {
            configurable: true,
            value: fakeDatabase,
          });
          request.onsuccess?.(new Event('success'));
        });

        return request;
      }),
    } as IDBFactory;

    const service = new IndexedDBService();

    await expect(service.init()).rejects.toThrow('open failed');
    await expect(service.init()).resolves.toBeUndefined();

    globalThis.indexedDB = originalIndexedDb;
  });

  it('deduplicates transcript segments by meeting and sequence id', async () => {
    const service = new IndexedDBService();
    await service.init();

    await service.saveMeetingMetadata({
      meetingId: 'meeting-1',
      title: 'Test meeting',
      startTime: Date.now(),
      lastUpdated: Date.now(),
      transcriptCount: 0,
      savedToSQLite: false,
    });

    const transcript = {
      text: 'hello world',
      timestamp: '2026-04-04T12:00:00Z',
      confidence: 0.92,
      sequence_id: 1,
      audio_start_time: 0,
      audio_end_time: 1,
      duration: 1,
    };

    await service.saveTranscript('meeting-1', transcript);
    await service.saveTranscript('meeting-1', transcript);

    expect(await service.getTranscriptCount('meeting-1')).toBe(1);
    const metadata = await service.getMeetingMetadata('meeting-1');
    expect(metadata?.transcriptCount).toBe(1);

    const transcripts = await service.getTranscripts('meeting-1');
    expect(transcripts).toHaveLength(1);
    expect(transcripts[0].sequenceId).toBe(1);
  });
});

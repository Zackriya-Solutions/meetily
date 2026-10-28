import { expect, test } from 'bun:test';
import 'fake-indexeddb/auto';
import { indexedDBService, type MeetingMetadata } from '../../src/services/indexedDBService';

function metadata(meetingId: string): MeetingMetadata {
  return { meetingId, title: 'Checkpoint test', startTime: Date.now(), lastUpdated: Date.now(),
    transcriptCount: 0, savedToSQLite: false };
}

async function readPersisted(meetingId: string): Promise<MeetingMetadata> {
  // Use a fresh connection rather than the service's in-memory saved-session set.
  const db = await new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open('MeetilyRecoveryDB');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  try {
    return await new Promise((resolve, reject) => {
      const request = db.transaction('meetings', 'readonly').objectStore('meetings').get(meetingId);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally { db.close(); }
}

test('late stale folder metadata cannot restore a saved meeting as recoverable', async () => {
  const original = metadata('stale-folder');
  await indexedDBService.saveMeetingMetadata(original);
  const stale = (await indexedDBService.getMeetingMetadata(original.meetingId))!;
  await indexedDBService.markMeetingSaved(original.meetingId);
  await indexedDBService.saveMeetingMetadata({ ...stale, folderPath: '/mock/meeting' });
  const persisted = await readPersisted(original.meetingId);
  expect(persisted.savedToSQLite).toBe(true); expect(persisted.folderPath).toBe('/mock/meeting');
  expect((await indexedDBService.getAllMeetings()).map(m => m.meetingId)).not.toContain(original.meetingId);
});

test('a very short recording completing before initialization still persists saved metadata', async () => {
  const original = metadata('completion-before-initialization');
  await indexedDBService.markMeetingSaved(original.meetingId);
  await indexedDBService.saveMeetingMetadata(original);
  expect((await readPersisted(original.meetingId)).savedToSQLite).toBe(true);
});

test('concurrent completion, folder updates, and final transcript writes preserve the saved flag and segment count', async () => {
  const original = metadata('concurrent-checkpoints');
  await indexedDBService.saveMeetingMetadata(original);
  await Promise.all([
    indexedDBService.saveTranscript(original.meetingId, { text: 'Last buffered words', sequenceId: 1 }),
    indexedDBService.markMeetingSaved(original.meetingId),
    indexedDBService.saveMeetingMetadata({ ...original, folderPath: '/mock/concurrent' }),
  ]);
  const persisted = await readPersisted(original.meetingId);
  expect(persisted.savedToSQLite).toBe(true); expect(persisted.transcriptCount).toBe(1);
  expect(await indexedDBService.getTranscripts(original.meetingId)).toHaveLength(1);
});

test('unsaved interrupted checkpoints remain available for recovery', async () => {
  const original = metadata('interrupted-checkpoints');
  await indexedDBService.saveMeetingMetadata(original);
  expect((await readPersisted(original.meetingId)).savedToSQLite).toBe(false);
  expect((await indexedDBService.getAllMeetings()).map(m => m.meetingId)).toContain(original.meetingId);
});

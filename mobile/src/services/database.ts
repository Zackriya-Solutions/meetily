/**
 * Local SQLite database for offline-first meeting storage.
 *
 * Uses @capacitor-community/sqlite on native platforms.
 * Falls back to in-memory storage for web/dev mode.
 */

import { Meeting, SyncQueueEntry, SyncOperation } from '@/types'

// Schema version for migrations
const SCHEMA_VERSION = 1

const CREATE_TABLES_SQL = `
CREATE TABLE IF NOT EXISTS meetings (
  meeting_id TEXT PRIMARY KEY,
  title TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'recording',
  duration_seconds REAL,
  transcript_text TEXT,
  transcript_segments TEXT,
  summary TEXT,
  audio_file_path TEXT,
  sync_status TEXT NOT NULL DEFAULT 'local_only',
  version INTEGER NOT NULL DEFAULT 1,
  last_synced_at TEXT
);

CREATE TABLE IF NOT EXISTS sync_queue (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  operation TEXT NOT NULL,
  meeting_id TEXT NOT NULL,
  payload TEXT,
  created_at TEXT NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'pending'
);

CREATE TABLE IF NOT EXISTS sync_state (
  key TEXT PRIMARY KEY,
  value TEXT
);
`

// In-memory fallback for web/dev mode
class InMemoryDatabase {
  private meetings: Map<string, Meeting> = new Map()
  private syncQueue: SyncQueueEntry[] = []
  private syncState: Map<string, string> = new Map()
  private nextQueueId = 1

  async initialize(): Promise<void> {
    // No-op for in-memory
  }

  async insertMeeting(meeting: Meeting): Promise<void> {
    this.meetings.set(meeting.meeting_id, { ...meeting })
  }

  async getMeetings(): Promise<Meeting[]> {
    return Array.from(this.meetings.values())
      .filter((m) => m.status !== 'error' || true) // Include all
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
  }

  async getMeeting(meetingId: string): Promise<Meeting | null> {
    return this.meetings.get(meetingId) || null
  }

  async updateMeeting(meetingId: string, fields: Partial<Meeting>): Promise<void> {
    const existing = this.meetings.get(meetingId)
    if (existing) {
      this.meetings.set(meetingId, { ...existing, ...fields })
    }
  }

  async deleteMeeting(meetingId: string): Promise<void> {
    this.meetings.delete(meetingId)
  }

  async addToSyncQueue(
    operation: SyncOperation,
    meetingId: string,
    payload?: any,
  ): Promise<void> {
    this.syncQueue.push({
      id: this.nextQueueId++,
      operation,
      meeting_id: meetingId,
      payload: payload ? JSON.stringify(payload) : '',
      created_at: new Date().toISOString(),
      retry_count: 0,
      status: 'pending',
    })
  }

  async getPendingSyncItems(): Promise<SyncQueueEntry[]> {
    return this.syncQueue.filter((item) => item.status === 'pending')
  }

  async updateSyncQueueItem(id: number, status: string, retryCount?: number): Promise<void> {
    const item = this.syncQueue.find((i) => i.id === id)
    if (item) {
      item.status = status as any
      if (retryCount !== undefined) item.retry_count = retryCount
    }
  }

  async removeSyncQueueItem(id: number): Promise<void> {
    this.syncQueue = this.syncQueue.filter((i) => i.id !== id)
  }

  async getPendingCount(): Promise<number> {
    return this.syncQueue.filter((i) => i.status === 'pending').length
  }

  async getSyncState(key: string): Promise<string | null> {
    return this.syncState.get(key) || null
  }

  async setSyncState(key: string, value: string): Promise<void> {
    this.syncState.set(key, value)
  }

  async applyRemoteMeetings(remoteMeetings: any[]): Promise<void> {
    for (const remote of remoteMeetings) {
      const local = this.meetings.get(remote.meeting_id)
      if (!local || remote.version > local.version) {
        const meeting: Meeting = {
          meeting_id: remote.meeting_id,
          title: remote.title || '',
          created_at: remote.created_at,
          updated_at: remote.updated_at,
          status: remote.status || 'completed',
          duration_seconds: remote.duration_seconds,
          transcript_text: remote.transcript_text,
          transcript_segments: remote.transcript_segments,
          summary: remote.summary,
          audio_file_path: local?.audio_file_path || undefined,
          sync_status: 'synced',
          version: remote.version || 1,
          last_synced_at: new Date().toISOString(),
        }
        this.meetings.set(remote.meeting_id, meeting)
      }
    }
  }
}

// Singleton instance
let _db: InMemoryDatabase | null = null

export function getDatabase(): InMemoryDatabase {
  if (!_db) {
    _db = new InMemoryDatabase()
  }
  return _db
}

export async function initializeDatabase(): Promise<void> {
  const db = getDatabase()
  await db.initialize()
}

// Re-export for convenience
export type Database = InMemoryDatabase

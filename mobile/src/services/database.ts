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

// Database interface shared by both implementations
interface DatabaseAdapter {
  initialize(): Promise<void>
  insertMeeting(meeting: Meeting): Promise<void>
  getMeetings(): Promise<Meeting[]>
  getMeeting(meetingId: string): Promise<Meeting | null>
  updateMeeting(meetingId: string, fields: Partial<Meeting>): Promise<void>
  deleteMeeting(meetingId: string): Promise<void>
  addToSyncQueue(operation: SyncOperation, meetingId: string, payload?: any): Promise<void>
  getPendingSyncItems(): Promise<SyncQueueEntry[]>
  updateSyncQueueItem(id: number, status: string, retryCount?: number): Promise<void>
  removeSyncQueueItem(id: number): Promise<void>
  getPendingCount(): Promise<number>
  getSyncState(key: string): Promise<string | null>
  setSyncState(key: string, value: string): Promise<void>
  applyRemoteMeetings(remoteMeetings: any[]): Promise<void>
}

// ── Capacitor SQLite adapter ────────────────────────────────────────

class CapacitorSQLiteDatabase implements DatabaseAdapter {
  private db: any = null
  private sqlite: any = null

  async initialize(): Promise<void> {
    try {
      const { CapacitorSQLite } = await import('@capacitor-community/sqlite')
      this.sqlite = CapacitorSQLite

      // Create/open database
      const ret = await this.sqlite.createConnection({
        database: 'iqcapture',
        version: SCHEMA_VERSION,
        encrypted: false,
        mode: 'no-encryption',
      })
      this.db = ret

      await this.sqlite.open({ database: 'iqcapture' })

      // Create tables
      const statements = CREATE_TABLES_SQL.split(';')
        .map(s => s.trim())
        .filter(s => s.length > 0)

      for (const stmt of statements) {
        await this.sqlite.execute({ database: 'iqcapture', statements: stmt + ';' })
      }

      console.log('[Database] Capacitor SQLite initialized')
    } catch (e) {
      console.error('[Database] Failed to initialize Capacitor SQLite:', e)
      throw e
    }
  }

  private async run(sql: string, values: any[] = []): Promise<any> {
    return this.sqlite.run({ database: 'iqcapture', statement: sql, values })
  }

  private async query(sql: string, values: any[] = []): Promise<any[]> {
    const result = await this.sqlite.query({ database: 'iqcapture', statement: sql, values })
    return result.values || []
  }

  async insertMeeting(meeting: Meeting): Promise<void> {
    await this.run(
      `INSERT OR REPLACE INTO meetings (meeting_id, title, created_at, updated_at, status, duration_seconds, transcript_text, transcript_segments, summary, audio_file_path, sync_status, version, last_synced_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        meeting.meeting_id, meeting.title, meeting.created_at, meeting.updated_at,
        meeting.status, meeting.duration_seconds || null,
        meeting.transcript_text || null,
        meeting.transcript_segments ? JSON.stringify(meeting.transcript_segments) : null,
        meeting.summary ? JSON.stringify(meeting.summary) : null,
        meeting.audio_file_path || null,
        meeting.sync_status, meeting.version, meeting.last_synced_at || null,
      ]
    )
  }

  async getMeetings(): Promise<Meeting[]> {
    const rows = await this.query('SELECT * FROM meetings ORDER BY created_at DESC')
    return rows.map(this.rowToMeeting)
  }

  async getMeeting(meetingId: string): Promise<Meeting | null> {
    const rows = await this.query('SELECT * FROM meetings WHERE meeting_id = ?', [meetingId])
    return rows.length > 0 ? this.rowToMeeting(rows[0]) : null
  }

  async updateMeeting(meetingId: string, fields: Partial<Meeting>): Promise<void> {
    const existing = await this.getMeeting(meetingId)
    if (!existing) return
    const merged = { ...existing, ...fields }
    await this.insertMeeting(merged)
  }

  async deleteMeeting(meetingId: string): Promise<void> {
    await this.run('DELETE FROM meetings WHERE meeting_id = ?', [meetingId])
  }

  async addToSyncQueue(operation: SyncOperation, meetingId: string, payload?: any): Promise<void> {
    await this.run(
      'INSERT INTO sync_queue (operation, meeting_id, payload, created_at) VALUES (?, ?, ?, ?)',
      [operation, meetingId, payload ? JSON.stringify(payload) : '', new Date().toISOString()]
    )
  }

  async getPendingSyncItems(): Promise<SyncQueueEntry[]> {
    const rows = await this.query('SELECT * FROM sync_queue WHERE status = ?', ['pending'])
    return rows as SyncQueueEntry[]
  }

  async updateSyncQueueItem(id: number, status: string, retryCount?: number): Promise<void> {
    if (retryCount !== undefined) {
      await this.run('UPDATE sync_queue SET status = ?, retry_count = ? WHERE id = ?', [status, retryCount, id])
    } else {
      await this.run('UPDATE sync_queue SET status = ? WHERE id = ?', [status, id])
    }
  }

  async removeSyncQueueItem(id: number): Promise<void> {
    await this.run('DELETE FROM sync_queue WHERE id = ?', [id])
  }

  async getPendingCount(): Promise<number> {
    const rows = await this.query('SELECT COUNT(*) as cnt FROM sync_queue WHERE status = ?', ['pending'])
    return rows[0]?.cnt || 0
  }

  async getSyncState(key: string): Promise<string | null> {
    const rows = await this.query('SELECT value FROM sync_state WHERE key = ?', [key])
    return rows.length > 0 ? rows[0].value : null
  }

  async setSyncState(key: string, value: string): Promise<void> {
    await this.run('INSERT OR REPLACE INTO sync_state (key, value) VALUES (?, ?)', [key, value])
  }

  async applyRemoteMeetings(remoteMeetings: any[]): Promise<void> {
    for (const remote of remoteMeetings) {
      const local = await this.getMeeting(remote.meeting_id)
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
        await this.insertMeeting(meeting)
      }
    }
  }

  private rowToMeeting(row: any): Meeting {
    return {
      meeting_id: row.meeting_id,
      title: row.title,
      created_at: row.created_at,
      updated_at: row.updated_at,
      status: row.status,
      duration_seconds: row.duration_seconds,
      transcript_text: row.transcript_text,
      transcript_segments: row.transcript_segments ? JSON.parse(row.transcript_segments) : undefined,
      summary: row.summary ? JSON.parse(row.summary) : undefined,
      audio_file_path: row.audio_file_path,
      sync_status: row.sync_status,
      version: row.version,
      last_synced_at: row.last_synced_at,
    }
  }
}

// ── In-memory fallback for web/dev mode ─────────────────────────────

class InMemoryDatabase implements DatabaseAdapter {
  private meetings: Map<string, Meeting> = new Map()
  private syncQueue: SyncQueueEntry[] = []
  private syncState: Map<string, string> = new Map()
  private nextQueueId = 1

  async initialize(): Promise<void> {
    console.log('[Database] Using in-memory fallback (web/dev mode)')
  }

  async insertMeeting(meeting: Meeting): Promise<void> {
    this.meetings.set(meeting.meeting_id, { ...meeting })
  }

  async getMeetings(): Promise<Meeting[]> {
    return Array.from(this.meetings.values())
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

  async addToSyncQueue(operation: SyncOperation, meetingId: string, payload?: any): Promise<void> {
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

// ── Database factory ────────────────────────────────────────────────

let _db: DatabaseAdapter | null = null
let _initPromise: Promise<void> | null = null

function isNativePlatform(): boolean {
  if (typeof window === 'undefined') return false
  return !!(window as any).Capacitor?.isNativePlatform?.()
}

export function getDatabase(): DatabaseAdapter {
  if (!_db) {
    // Pre-initialization fallback — will be replaced by initializeDatabase()
    _db = new InMemoryDatabase()
  }
  return _db
}

export async function initializeDatabase(): Promise<void> {
  if (_initPromise) return _initPromise

  _initPromise = (async () => {
    if (isNativePlatform()) {
      try {
        const sqliteDb = new CapacitorSQLiteDatabase()
        await sqliteDb.initialize()
        _db = sqliteDb
        console.log('[Database] Capacitor SQLite ready')
        return
      } catch (e) {
        console.warn('[Database] Capacitor SQLite failed, falling back to in-memory:', e)
      }
    }

    // Fallback
    const memDb = new InMemoryDatabase()
    await memDb.initialize()
    _db = memDb
  })()

  return _initPromise
}

// Re-export for convenience
export type Database = DatabaseAdapter

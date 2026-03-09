import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// Mock database
const mockDb = {
  getPendingSyncItems: vi.fn().mockResolvedValue([]),
  updateSyncQueueItem: vi.fn().mockResolvedValue(undefined),
  removeSyncQueueItem: vi.fn().mockResolvedValue(undefined),
  updateMeeting: vi.fn().mockResolvedValue(undefined),
  getSyncState: vi.fn().mockResolvedValue(null),
  setSyncState: vi.fn().mockResolvedValue(undefined),
  applyRemoteMeetings: vi.fn().mockResolvedValue(undefined),
}

vi.mock('./database', () => ({
  getDatabase: () => mockDb,
}))

// Mock authService
vi.mock('./authService', () => ({
  getAccessToken: vi.fn().mockResolvedValue('mock-token'),
  authFetch: vi.fn().mockResolvedValue({
    ok: true,
    json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
    status: 200,
  }),
}))

// Mock config
vi.mock('./config', () => ({
  config: {
    apiUrl: 'https://api.test.com',
  },
}))

import { syncService } from './syncService'
import { getAccessToken, authFetch } from './authService'

describe('SyncService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
    // Ensure navigator.onLine is true
    Object.defineProperty(navigator, 'onLine', { value: true, writable: true })
  })

  afterEach(() => {
    syncService.stop()
    vi.useRealTimers()
  })

  describe('start / stop', () => {
    it('start sets up interval and stop clears it', () => {
      const clearSpy = vi.spyOn(global, 'clearInterval')

      syncService.start()
      // Calling start again should be a no-op (idempotent)
      syncService.start()

      syncService.stop()
      expect(clearSpy).toHaveBeenCalled()

      // Stopping again should be safe
      syncService.stop()
    })

    it('start triggers initial sync after delay', async () => {
      vi.mocked(getAccessToken).mockResolvedValue('token')
      vi.mocked(authFetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
      } as Response)

      syncService.start()

      // Advance past the 2s initial delay and flush microtasks
      await vi.advanceTimersByTimeAsync(2100)

      syncService.stop()
    })
  })

  describe('sync', () => {
    it('skips when offline', async () => {
      Object.defineProperty(navigator, 'onLine', { value: false, writable: true })

      await syncService.sync()

      expect(getAccessToken).not.toHaveBeenCalled()
    })

    it('skips when not authenticated', async () => {
      vi.mocked(getAccessToken).mockResolvedValueOnce(null)

      await syncService.sync()

      expect(mockDb.getPendingSyncItems).not.toHaveBeenCalled()
    })

    it('processes queue items and syncs meetings on successful sync', async () => {
      mockDb.getPendingSyncItems.mockResolvedValueOnce([])
      vi.mocked(authFetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
      } as Response)

      await syncService.sync()

      expect(mockDb.getPendingSyncItems).toHaveBeenCalled()
      // syncMeetings should be called
      expect(authFetch).toHaveBeenCalledWith('/api/meetings/sync', expect.any(Object))
    })

    it('does not run concurrently (guards with isSyncing)', async () => {
      // Make the sync take a while
      let resolveSync: () => void
      mockDb.getPendingSyncItems.mockReturnValueOnce(
        new Promise((resolve) => {
          resolveSync = () => resolve([])
        }),
      )

      const sync1 = syncService.sync()
      // Flush microtasks so sync1 passes the await getAccessToken() and sets isSyncing = true
      await Promise.resolve()
      await Promise.resolve()

      const sync2 = syncService.sync() // Should be skipped (isSyncing is true)

      resolveSync!()
      await sync1
      await sync2

      // getPendingSyncItems should only be called once
      expect(mockDb.getPendingSyncItems).toHaveBeenCalledTimes(1)
    })

    it('catches errors during sync and resets isSyncing', async () => {
      mockDb.getPendingSyncItems.mockRejectedValueOnce(new Error('DB error'))
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      await syncService.sync()

      expect(warnSpy).toHaveBeenCalled()

      // Should be able to sync again (isSyncing reset)
      mockDb.getPendingSyncItems.mockResolvedValueOnce([])
      await syncService.sync()
      expect(mockDb.getPendingSyncItems).toHaveBeenCalledTimes(2)

      warnSpy.mockRestore()
    })
  })

  describe('processQueueItem (via sync)', () => {
    it('handles create operation', async () => {
      const item = {
        id: 1,
        operation: 'create',
        meeting_id: 'meeting-1',
        payload: JSON.stringify({ title: 'Standup', duration_seconds: 3600 }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 0,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string, options?: any) => {
        if (path === '/api/meetings') {
          return {
            ok: true,
            json: () => Promise.resolve({ meeting_id: 'meeting-1' }),
          } as Response
        }
        // syncMeetings call
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      await syncService.sync()

      expect(mockDb.updateSyncQueueItem).toHaveBeenCalledWith(1, 'in_progress')
      expect(authFetch).toHaveBeenCalledWith('/api/meetings', expect.objectContaining({
        method: 'POST',
      }))
      expect(mockDb.updateMeeting).toHaveBeenCalledWith('meeting-1', expect.objectContaining({
        sync_status: 'synced',
      }))
      expect(mockDb.removeSyncQueueItem).toHaveBeenCalledWith(1)
    })

    it('handles update operation', async () => {
      const item = {
        id: 2,
        operation: 'update',
        meeting_id: 'meeting-2',
        payload: JSON.stringify({ title: 'Updated' }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 0,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string) => {
        if (path.includes('/api/meetings/meeting-2')) {
          return { ok: true, json: () => Promise.resolve({}) } as Response
        }
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      await syncService.sync()

      expect(authFetch).toHaveBeenCalledWith('/api/meetings/meeting-2', expect.objectContaining({
        method: 'PUT',
      }))
      expect(mockDb.removeSyncQueueItem).toHaveBeenCalledWith(2)
    })

    it('handles delete operation', async () => {
      const item = {
        id: 3,
        operation: 'delete',
        meeting_id: 'meeting-3',
        payload: '',
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 0,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string) => {
        if (path.includes('/api/meetings/meeting-3')) {
          return { ok: true, json: () => Promise.resolve({}) } as Response
        }
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      await syncService.sync()

      expect(authFetch).toHaveBeenCalledWith('/api/meetings/meeting-3', expect.objectContaining({
        method: 'DELETE',
      }))
      expect(mockDb.removeSyncQueueItem).toHaveBeenCalledWith(3)
    })

    it('handles upload_audio operation', async () => {
      const item = {
        id: 4,
        operation: 'upload_audio',
        meeting_id: 'meeting-4',
        payload: JSON.stringify({ audio_file_path: '/path/audio.webm', provider: 'deepgram' }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 0,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      const mockBlob = new Blob(['audio-data'])
      const mockFetchFn = vi.fn()
        // fetch audio file
        .mockResolvedValueOnce({ ok: true, blob: () => Promise.resolve(mockBlob) })
        // upload to API
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ transcription_id: 'txn-1' }),
        })
      global.fetch = mockFetchFn

      vi.mocked(authFetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
      } as Response)

      await syncService.sync()

      expect(mockFetchFn).toHaveBeenCalledWith('/path/audio.webm')
      expect(mockFetchFn).toHaveBeenCalledWith(
        'https://api.test.com/api/transcription/upload',
        expect.objectContaining({ method: 'POST' }),
      )
      expect(mockDb.updateMeeting).toHaveBeenCalledWith('meeting-4', {
        status: 'transcribing',
        sync_status: 'synced',
      })
      expect(mockDb.removeSyncQueueItem).toHaveBeenCalledWith(4)
    })

    it('handles request_summary operation', async () => {
      const item = {
        id: 5,
        operation: 'request_summary',
        meeting_id: 'meeting-5',
        payload: JSON.stringify({ provider: 'claude', model: 'claude-3' }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 0,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string) => {
        if (path === '/api/summarize') {
          return { ok: true, json: () => Promise.resolve({}) } as Response
        }
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      await syncService.sync()

      expect(authFetch).toHaveBeenCalledWith('/api/summarize', expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('meeting-5'),
      }))
      expect(mockDb.updateMeeting).toHaveBeenCalledWith('meeting-5', { status: 'summarizing' })
      expect(mockDb.removeSyncQueueItem).toHaveBeenCalledWith(5)
    })

    it('retries on failure and marks as failed after MAX_RETRIES', async () => {
      const item = {
        id: 10,
        operation: 'create',
        meeting_id: 'meeting-fail',
        payload: JSON.stringify({ title: 'Fail' }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 4, // One more retry will hit MAX_RETRIES (5)
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string) => {
        if (path === '/api/meetings') {
          return { ok: false, status: 500 } as Response
        }
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      await syncService.sync()

      // Should be marked as failed (retry_count 4 + 1 = 5 >= MAX_RETRIES)
      expect(mockDb.updateSyncQueueItem).toHaveBeenCalledWith(10, 'failed', 5)
      expect(mockDb.removeSyncQueueItem).not.toHaveBeenCalledWith(10)

      warnSpy.mockRestore()
    })

    it('re-queues as pending when retries remain', async () => {
      const item = {
        id: 11,
        operation: 'create',
        meeting_id: 'meeting-retry',
        payload: JSON.stringify({ title: 'Retry' }),
        created_at: '2025-01-01T00:00:00Z',
        retry_count: 1,
        status: 'pending',
      }
      mockDb.getPendingSyncItems.mockResolvedValueOnce([item])

      vi.mocked(authFetch).mockImplementation(async (path: string) => {
        if (path === '/api/meetings') {
          return { ok: false, status: 500 } as Response
        }
        return {
          ok: true,
          json: () => Promise.resolve({ remote_changes: [], server_time: '2025-01-01T00:00:00Z' }),
        } as Response
      })

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      await syncService.sync()

      expect(mockDb.updateSyncQueueItem).toHaveBeenCalledWith(11, 'pending', 2)

      warnSpy.mockRestore()
    })
  })

  describe('syncMeetings', () => {
    it('calls /api/meetings/sync and applies remote changes', async () => {
      const remoteChanges = [
        { meeting_id: 'remote-1', title: 'Remote Meeting', version: 2 },
      ]
      mockDb.getPendingSyncItems.mockResolvedValueOnce([])
      mockDb.getSyncState.mockResolvedValueOnce('2024-12-01T00:00:00Z')

      vi.mocked(authFetch).mockResolvedValue({
        ok: true,
        json: () =>
          Promise.resolve({
            remote_changes: remoteChanges,
            server_time: '2025-01-01T12:00:00Z',
          }),
      } as Response)

      await syncService.sync()

      expect(authFetch).toHaveBeenCalledWith('/api/meetings/sync', {
        method: 'POST',
        body: JSON.stringify({
          last_sync_at: '2024-12-01T00:00:00Z',
          local_changes: [],
        }),
      })
      expect(mockDb.applyRemoteMeetings).toHaveBeenCalledWith(remoteChanges)
      expect(mockDb.setSyncState).toHaveBeenCalledWith('last_sync_at', '2025-01-01T12:00:00Z')
    })

    it('does not apply changes when response is not ok', async () => {
      mockDb.getPendingSyncItems.mockResolvedValueOnce([])

      vi.mocked(authFetch).mockResolvedValue({
        ok: false,
        status: 500,
      } as Response)

      await syncService.sync()

      expect(mockDb.applyRemoteMeetings).not.toHaveBeenCalled()
    })

    it('does not apply when remote_changes is empty', async () => {
      mockDb.getPendingSyncItems.mockResolvedValueOnce([])

      vi.mocked(authFetch).mockResolvedValue({
        ok: true,
        json: () =>
          Promise.resolve({
            remote_changes: [],
            server_time: '2025-01-01T12:00:00Z',
          }),
      } as Response)

      await syncService.sync()

      expect(mockDb.applyRemoteMeetings).not.toHaveBeenCalled()
      expect(mockDb.setSyncState).toHaveBeenCalledWith('last_sync_at', '2025-01-01T12:00:00Z')
    })
  })
})

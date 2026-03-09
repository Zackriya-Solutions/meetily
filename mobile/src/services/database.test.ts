import { describe, it, expect, vi, beforeEach } from 'vitest'
import { Meeting, SyncOperation } from '@/types'

describe('database (InMemoryDatabase fallback)', () => {
  // Since @capacitor-community/sqlite is mocked to throw in setup.ts,
  // initializeDatabase() will always fall back to InMemoryDatabase.

  let db: any
  let initializeDatabase: any
  let getDatabase: any

  beforeEach(async () => {
    vi.resetModules()
    const mod = await import('./database')
    initializeDatabase = mod.initializeDatabase
    getDatabase = mod.getDatabase
    await initializeDatabase()
    db = getDatabase()
  })

  function makeMeeting(overrides: Partial<Meeting> = {}): Meeting {
    return {
      meeting_id: 'meet-1',
      title: 'Test Meeting',
      created_at: '2024-06-01T10:00:00Z',
      updated_at: '2024-06-01T10:00:00Z',
      status: 'completed',
      sync_status: 'local_only',
      version: 1,
      ...overrides,
    }
  }

  describe('initializeDatabase()', () => {
    it('falls back to in-memory database when SQLite is unavailable', async () => {
      // Already initialized in beforeEach — just verify it works
      expect(db).toBeDefined()
      const meetings = await db.getMeetings()
      expect(meetings).toEqual([])
    })

    it('returns the same promise if called multiple times', async () => {
      vi.resetModules()
      const mod = await import('./database')
      // Both calls should succeed and only initialize once.
      // Since initializeDatabase is async, each call wraps the internal
      // _initPromise in a new async-function Promise, so we verify both
      // resolve without double-initialization instead of reference equality.
      const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
      await Promise.all([mod.initializeDatabase(), mod.initializeDatabase()])
      // The in-memory fallback logs once on init — should only happen once
      const initLogs = consoleSpy.mock.calls.filter(
        (args) => typeof args[0] === 'string' && args[0].includes('[Database]'),
      )
      expect(initLogs).toHaveLength(1)
      consoleSpy.mockRestore()
    })
  })

  describe('getDatabase()', () => {
    it('returns an InMemoryDatabase instance before init', async () => {
      vi.resetModules()
      const mod = await import('./database')
      const preInitDb = mod.getDatabase()
      expect(preInitDb).toBeDefined()
      // Should still work
      const meetings = await preInitDb.getMeetings()
      expect(meetings).toEqual([])
    })
  })

  describe('insertMeeting / getMeeting / getMeetings', () => {
    it('inserts and retrieves a meeting by ID', async () => {
      const meeting = makeMeeting()
      await db.insertMeeting(meeting)

      const retrieved = await db.getMeeting('meet-1')
      expect(retrieved).toEqual(meeting)
    })

    it('returns null for non-existent meeting', async () => {
      const result = await db.getMeeting('nonexistent')
      expect(result).toBeNull()
    })

    it('getMeetings returns all meetings', async () => {
      await db.insertMeeting(makeMeeting({ meeting_id: 'm1' }))
      await db.insertMeeting(makeMeeting({ meeting_id: 'm2' }))

      const meetings = await db.getMeetings()
      expect(meetings).toHaveLength(2)
    })

    it('getMeetings returns sorted by created_at DESC', async () => {
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'm-old', created_at: '2024-01-01T00:00:00Z' }),
      )
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'm-new', created_at: '2024-12-01T00:00:00Z' }),
      )
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'm-mid', created_at: '2024-06-01T00:00:00Z' }),
      )

      const meetings = await db.getMeetings()
      expect(meetings[0].meeting_id).toBe('m-new')
      expect(meetings[1].meeting_id).toBe('m-mid')
      expect(meetings[2].meeting_id).toBe('m-old')
    })

    it('insertMeeting overwrites existing meeting with same ID', async () => {
      await db.insertMeeting(makeMeeting({ meeting_id: 'm1', title: 'Original' }))
      await db.insertMeeting(makeMeeting({ meeting_id: 'm1', title: 'Updated' }))

      const meetings = await db.getMeetings()
      expect(meetings).toHaveLength(1)
      expect(meetings[0].title).toBe('Updated')
    })
  })

  describe('updateMeeting', () => {
    it('updates specific fields of an existing meeting', async () => {
      await db.insertMeeting(makeMeeting({ meeting_id: 'm1', title: 'Old Title' }))

      await db.updateMeeting('m1', { title: 'New Title', status: 'recording' as const })

      const updated = await db.getMeeting('m1')
      expect(updated!.title).toBe('New Title')
      expect(updated!.status).toBe('recording')
      // Other fields preserved
      expect(updated!.meeting_id).toBe('m1')
      expect(updated!.version).toBe(1)
    })

    it('does nothing for non-existent meeting', async () => {
      await db.updateMeeting('nonexistent', { title: 'New' })
      const result = await db.getMeeting('nonexistent')
      expect(result).toBeNull()
    })
  })

  describe('deleteMeeting', () => {
    it('removes a meeting', async () => {
      await db.insertMeeting(makeMeeting({ meeting_id: 'm1' }))
      await db.deleteMeeting('m1')

      const result = await db.getMeeting('m1')
      expect(result).toBeNull()
    })

    it('does not throw for non-existent meeting', async () => {
      await expect(db.deleteMeeting('nonexistent')).resolves.toBeUndefined()
    })
  })

  describe('sync queue operations', () => {
    it('addToSyncQueue creates a pending item', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1', { foo: 'bar' })

      const items = await db.getPendingSyncItems()
      expect(items).toHaveLength(1)
      expect(items[0].operation).toBe('create')
      expect(items[0].meeting_id).toBe('m1')
      expect(items[0].status).toBe('pending')
      expect(items[0].retry_count).toBe(0)
      expect(items[0].id).toBe(1)
    })

    it('getPendingSyncItems returns only pending items', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      await db.addToSyncQueue('update' as SyncOperation, 'm2')

      // Mark one as failed
      const items = await db.getPendingSyncItems()
      await db.updateSyncQueueItem(items[0].id, 'failed')

      const pending = await db.getPendingSyncItems()
      expect(pending).toHaveLength(1)
      expect(pending[0].meeting_id).toBe('m2')
    })

    it('updateSyncQueueItem updates status and retry count', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      const items = await db.getPendingSyncItems()

      await db.updateSyncQueueItem(items[0].id, 'in_progress', 3)

      const pending = await db.getPendingSyncItems()
      expect(pending).toHaveLength(0)

      // getPendingCount should be 0 after status change
      const count = await db.getPendingCount()
      expect(count).toBe(0)
    })

    it('updateSyncQueueItem updates only status when retryCount is omitted', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      const items = await db.getPendingSyncItems()

      await db.updateSyncQueueItem(items[0].id, 'failed')

      // retry_count should remain 0
      // We can check by adding another item and looking at all via getPendingSyncItems
      // after changing status back
      // Actually let's just re-mark as pending and check
      await db.updateSyncQueueItem(items[0].id, 'pending')
      const pending = await db.getPendingSyncItems()
      expect(pending[0].retry_count).toBe(0)
    })

    it('removeSyncQueueItem removes an item', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      await db.addToSyncQueue('update' as SyncOperation, 'm2')

      const items = await db.getPendingSyncItems()
      await db.removeSyncQueueItem(items[0].id)

      const remaining = await db.getPendingSyncItems()
      expect(remaining).toHaveLength(1)
      expect(remaining[0].meeting_id).toBe('m2')
    })

    it('getPendingCount returns correct count', async () => {
      expect(await db.getPendingCount()).toBe(0)

      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      await db.addToSyncQueue('update' as SyncOperation, 'm2')
      await db.addToSyncQueue('delete' as SyncOperation, 'm3')

      expect(await db.getPendingCount()).toBe(3)

      // Mark one as done
      const items = await db.getPendingSyncItems()
      await db.updateSyncQueueItem(items[0].id, 'completed')
      expect(await db.getPendingCount()).toBe(2)
    })

    it('addToSyncQueue auto-increments IDs', async () => {
      await db.addToSyncQueue('create' as SyncOperation, 'm1')
      await db.addToSyncQueue('create' as SyncOperation, 'm2')

      const items = await db.getPendingSyncItems()
      expect(items[0].id).toBe(1)
      expect(items[1].id).toBe(2)
    })

    it('addToSyncQueue stores payload as JSON string', async () => {
      await db.addToSyncQueue('update' as SyncOperation, 'm1', { title: 'new' })

      const items = await db.getPendingSyncItems()
      expect(items[0].payload).toBe(JSON.stringify({ title: 'new' }))
    })

    it('addToSyncQueue stores empty string when no payload', async () => {
      await db.addToSyncQueue('delete' as SyncOperation, 'm1')

      const items = await db.getPendingSyncItems()
      expect(items[0].payload).toBe('')
    })
  })

  describe('sync state operations', () => {
    it('getSyncState returns null for unknown key', async () => {
      const result = await db.getSyncState('unknown')
      expect(result).toBeNull()
    })

    it('setSyncState stores and getSyncState retrieves', async () => {
      await db.setSyncState('last_sync', '2024-06-01T00:00:00Z')

      const value = await db.getSyncState('last_sync')
      expect(value).toBe('2024-06-01T00:00:00Z')
    })

    it('setSyncState overwrites existing value', async () => {
      await db.setSyncState('cursor', 'abc')
      await db.setSyncState('cursor', 'def')

      const value = await db.getSyncState('cursor')
      expect(value).toBe('def')
    })
  })

  describe('applyRemoteMeetings()', () => {
    it('inserts a remote meeting that does not exist locally', async () => {
      const remoteMeetings = [
        {
          meeting_id: 'remote-1',
          title: 'Remote Meeting',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
          status: 'completed',
          version: 1,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('remote-1')
      expect(meeting).not.toBeNull()
      expect(meeting!.title).toBe('Remote Meeting')
      expect(meeting!.sync_status).toBe('synced')
    })

    it('updates local meeting when remote version is higher', async () => {
      await db.insertMeeting(
        makeMeeting({
          meeting_id: 'm1',
          title: 'Local Title',
          version: 1,
          audio_file_path: '/local/audio.webm',
        }),
      )

      const remoteMeetings = [
        {
          meeting_id: 'm1',
          title: 'Remote Title',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-02T00:00:00Z',
          status: 'completed',
          version: 2,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('m1')
      expect(meeting!.title).toBe('Remote Title')
      expect(meeting!.version).toBe(2)
      expect(meeting!.sync_status).toBe('synced')
      // Preserves local audio_file_path
      expect(meeting!.audio_file_path).toBe('/local/audio.webm')
    })

    it('does NOT overwrite local meeting when remote version is equal', async () => {
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'm1', title: 'Local Title', version: 3 }),
      )

      const remoteMeetings = [
        {
          meeting_id: 'm1',
          title: 'Remote Title',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
          version: 3,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('m1')
      expect(meeting!.title).toBe('Local Title')
    })

    it('does NOT overwrite local meeting when remote version is lower', async () => {
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'm1', title: 'Local Title', version: 5 }),
      )

      const remoteMeetings = [
        {
          meeting_id: 'm1',
          title: 'Old Remote',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
          version: 2,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('m1')
      expect(meeting!.title).toBe('Local Title')
      expect(meeting!.version).toBe(5)
    })

    it('defaults to version 1 if remote has no version', async () => {
      const remoteMeetings = [
        {
          meeting_id: 'r1',
          title: 'No Version',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('r1')
      expect(meeting!.version).toBe(1)
    })

    it('defaults to "completed" status if remote has no status', async () => {
      const remoteMeetings = [
        {
          meeting_id: 'r2',
          title: 'No Status',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
          version: 1,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('r2')
      expect(meeting!.status).toBe('completed')
    })

    it('handles multiple remote meetings', async () => {
      await db.insertMeeting(
        makeMeeting({ meeting_id: 'existing', title: 'Old', version: 1 }),
      )

      const remoteMeetings = [
        {
          meeting_id: 'existing',
          title: 'Updated',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-02T00:00:00Z',
          version: 2,
        },
        {
          meeting_id: 'brand-new',
          title: 'New Meeting',
          created_at: '2024-06-03T00:00:00Z',
          updated_at: '2024-06-03T00:00:00Z',
          version: 1,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const existing = await db.getMeeting('existing')
      expect(existing!.title).toBe('Updated')

      const brandNew = await db.getMeeting('brand-new')
      expect(brandNew).not.toBeNull()
      expect(brandNew!.title).toBe('New Meeting')
    })

    it('sets last_synced_at on applied meetings', async () => {
      const remoteMeetings = [
        {
          meeting_id: 'synced-1',
          title: 'Synced',
          created_at: '2024-06-01T00:00:00Z',
          updated_at: '2024-06-01T00:00:00Z',
          version: 1,
        },
      ]

      await db.applyRemoteMeetings(remoteMeetings)

      const meeting = await db.getMeeting('synced-1')
      expect(meeting!.last_synced_at).toBeDefined()
      // Should be a valid ISO string
      expect(new Date(meeting!.last_synced_at!).getTime()).not.toBeNaN()
    })
  })
})

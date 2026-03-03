/**
 * Sync engine — processes the sync queue and synchronizes with cloud API.
 *
 * Listens for connectivity changes and processes pending operations
 * (meeting creates/updates, audio uploads, transcription/summary requests).
 */

import { getDatabase } from './database'
import { authFetch, getAccessToken } from './authService'
import { SyncQueueEntry } from '@/types'

const SYNC_INTERVAL_MS = 60_000 // Sync every 60 seconds when online
const MAX_RETRIES = 5

class SyncService {
  private syncInterval: ReturnType<typeof setInterval> | null = null
  private isSyncing = false

  /**
   * Start periodic sync. Call once on app mount.
   */
  start(): void {
    if (this.syncInterval) return
    this.syncInterval = setInterval(() => this.sync(), SYNC_INTERVAL_MS)
    // Initial sync after a short delay
    setTimeout(() => this.sync(), 2000)
  }

  stop(): void {
    if (this.syncInterval) {
      clearInterval(this.syncInterval)
      this.syncInterval = null
    }
  }

  /**
   * Run a full sync cycle:
   * 1. Process pending sync queue items (uploads, requests)
   * 2. Push local changes to cloud
   * 3. Pull remote changes from cloud
   */
  async sync(): Promise<void> {
    if (this.isSyncing) return
    if (!navigator.onLine) return

    const token = await getAccessToken()
    if (!token) return // Not authenticated

    this.isSyncing = true
    try {
      await this.processSyncQueue()
      await this.syncMeetings()
    } catch (e) {
      console.warn('[SyncService] Sync failed:', e)
    } finally {
      this.isSyncing = false
    }
  }

  /**
   * Process pending items in the sync queue.
   */
  private async processSyncQueue(): Promise<void> {
    const db = getDatabase()
    const pending = await db.getPendingSyncItems()

    for (const item of pending) {
      try {
        await db.updateSyncQueueItem(item.id, 'in_progress')
        await this.processQueueItem(item)
        await db.removeSyncQueueItem(item.id)
      } catch (e) {
        console.warn(`[SyncService] Queue item ${item.id} failed:`, e)
        const newRetry = item.retry_count + 1
        if (newRetry >= MAX_RETRIES) {
          await db.updateSyncQueueItem(item.id, 'failed', newRetry)
        } else {
          await db.updateSyncQueueItem(item.id, 'pending', newRetry)
        }
      }
    }
  }

  /**
   * Process a single sync queue item.
   */
  private async processQueueItem(item: SyncQueueEntry): Promise<void> {
    const payload = item.payload ? JSON.parse(item.payload) : {}

    switch (item.operation) {
      case 'create': {
        const res = await authFetch('/api/meetings', {
          method: 'POST',
          body: JSON.stringify({
            title: payload.title || '',
            duration_seconds: payload.duration_seconds,
          }),
        })
        if (!res.ok) throw new Error(`Create failed: ${res.status}`)

        // Update local meeting with server data
        const db = getDatabase()
        const serverMeeting = await res.json()
        await db.updateMeeting(item.meeting_id, {
          sync_status: 'synced',
          last_synced_at: new Date().toISOString(),
        })
        break
      }

      case 'update': {
        const res = await authFetch(`/api/meetings/${item.meeting_id}`, {
          method: 'PUT',
          body: JSON.stringify(payload),
        })
        if (!res.ok && res.status !== 404) throw new Error(`Update failed: ${res.status}`)

        const db = getDatabase()
        await db.updateMeeting(item.meeting_id, {
          sync_status: 'synced',
          last_synced_at: new Date().toISOString(),
        })
        break
      }

      case 'delete': {
        const res = await authFetch(`/api/meetings/${item.meeting_id}`, {
          method: 'DELETE',
        })
        if (!res.ok && res.status !== 404) throw new Error(`Delete failed: ${res.status}`)
        break
      }

      case 'upload_audio': {
        // Upload audio file to cloud transcription endpoint
        const audioPath = payload.audio_file_path
        if (!audioPath) throw new Error('No audio file path')

        // Read file and create FormData
        const response = await fetch(audioPath)
        const blob = await response.blob()
        const formData = new FormData()
        formData.append('audio', blob, 'recording.m4a')
        formData.append('meeting_id', item.meeting_id)
        formData.append('provider', payload.provider || 'deepgram')

        const token = await getAccessToken()
        const baseUrl = process.env.NEXT_PUBLIC_CLOUD_API_URL || ''
        const uploadRes = await fetch(`${baseUrl}/api/transcription/upload`, {
          method: 'POST',
          headers: { Authorization: `Bearer ${token}` },
          body: formData,
        })

        if (!uploadRes.ok) throw new Error(`Upload failed: ${uploadRes.status}`)

        const uploadData = await uploadRes.json()
        // Queue transcription polling (handled by transcription service)
        const db = getDatabase()
        await db.updateMeeting(item.meeting_id, {
          status: 'transcribing',
          sync_status: 'synced',
        })
        break
      }

      case 'request_transcription': {
        // This is handled by upload_audio — transcription starts automatically
        break
      }

      case 'request_summary': {
        const res = await authFetch('/api/summarize', {
          method: 'POST',
          body: JSON.stringify({
            meeting_id: item.meeting_id,
            provider: payload.provider || 'claude',
            model: payload.model,
            custom_prompt: payload.custom_prompt,
          }),
        })
        if (!res.ok) throw new Error(`Summary request failed: ${res.status}`)

        const db = getDatabase()
        await db.updateMeeting(item.meeting_id, { status: 'summarizing' })
        break
      }
    }
  }

  /**
   * Sync meetings with cloud — push local changes, pull remote changes.
   */
  private async syncMeetings(): Promise<void> {
    const db = getDatabase()
    const lastSync = await db.getSyncState('last_sync_at')

    try {
      const res = await authFetch('/api/meetings/sync', {
        method: 'POST',
        body: JSON.stringify({
          last_sync_at: lastSync,
          local_changes: [], // Changes are sent via sync_queue individually
        }),
      })

      if (!res.ok) return

      const data = await res.json()

      // Apply remote changes to local SQLite
      if (data.remote_changes && data.remote_changes.length > 0) {
        await db.applyRemoteMeetings(data.remote_changes)
      }

      // Update last sync time
      await db.setSyncState('last_sync_at', data.server_time)
    } catch (e) {
      console.warn('[SyncService] Meeting sync failed:', e)
    }
  }
}

export const syncService = new SyncService()

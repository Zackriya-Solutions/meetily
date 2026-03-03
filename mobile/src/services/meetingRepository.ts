/**
 * Meeting repository — offline-first data access layer.
 *
 * All reads come from local SQLite (instant, works offline).
 * All writes go to local SQLite first, then queue sync to cloud.
 */

import { Meeting, SyncOperation } from '@/types'
import { getDatabase } from './database'
import { v4 as uuidv4 } from 'uuid'

class MeetingRepository {
  /**
   * Get all meetings from local database.
   */
  async getMeetings(): Promise<Meeting[]> {
    const db = getDatabase()
    return db.getMeetings()
  }

  /**
   * Get a single meeting by ID from local database.
   */
  async getMeeting(meetingId: string): Promise<Meeting | null> {
    const db = getDatabase()
    return db.getMeeting(meetingId)
  }

  /**
   * Create a meeting locally and queue sync to cloud.
   */
  async createMeeting(title: string, durationSeconds?: number): Promise<Meeting> {
    const db = getDatabase()
    const now = new Date().toISOString()
    const meetingId = uuidv4()

    const meeting: Meeting = {
      meeting_id: meetingId,
      title: title || 'Untitled Meeting',
      created_at: now,
      updated_at: now,
      status: 'recording',
      duration_seconds: durationSeconds,
      sync_status: 'local_only',
      version: 1,
    }

    await db.insertMeeting(meeting)
    await db.addToSyncQueue('create', meetingId, {
      title: meeting.title,
      duration_seconds: durationSeconds,
      created_at: now,
    })

    return meeting
  }

  /**
   * Update a meeting locally and queue sync.
   */
  async updateMeeting(meetingId: string, data: Partial<Meeting>): Promise<void> {
    const db = getDatabase()
    await db.updateMeeting(meetingId, {
      ...data,
      updated_at: new Date().toISOString(),
      sync_status: 'pending_sync',
    })
    await db.addToSyncQueue('update', meetingId, data)
  }

  /**
   * Delete a meeting locally and queue sync.
   */
  async deleteMeeting(meetingId: string): Promise<void> {
    const db = getDatabase()
    await db.deleteMeeting(meetingId)
    await db.addToSyncQueue('delete', meetingId)
  }

  /**
   * Queue an audio upload for a meeting. The sync engine will handle
   * the actual upload when connectivity is available.
   */
  async queueAudioUpload(meetingId: string, audioFilePath: string): Promise<void> {
    const db = getDatabase()
    await db.updateMeeting(meetingId, {
      status: 'pending_upload',
      audio_file_path: audioFilePath,
    })
    await db.addToSyncQueue('upload_audio', meetingId, {
      audio_file_path: audioFilePath,
      provider: 'deepgram',
    })
  }

  /**
   * Queue a summary generation request.
   */
  async requestSummary(
    meetingId: string,
    provider: string = 'claude',
    model?: string,
  ): Promise<void> {
    const db = getDatabase()
    await db.updateMeeting(meetingId, { status: 'summarizing' })
    await db.addToSyncQueue('request_summary', meetingId, { provider, model })
  }

  /**
   * Get count of pending sync items.
   */
  async getPendingCount(): Promise<number> {
    const db = getDatabase()
    return db.getPendingCount()
  }
}

export const meetingRepository = new MeetingRepository()

import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock uuid
vi.mock('uuid', () => ({ v4: () => 'mock-uuid-1234' }))

// Mock database module
const mockDb = {
  getMeetings: vi.fn().mockResolvedValue([]),
  getMeeting: vi.fn().mockResolvedValue(null),
  insertMeeting: vi.fn().mockResolvedValue(undefined),
  updateMeeting: vi.fn().mockResolvedValue(undefined),
  deleteMeeting: vi.fn().mockResolvedValue(undefined),
  addToSyncQueue: vi.fn().mockResolvedValue(undefined),
  getPendingCount: vi.fn().mockResolvedValue(0),
}

vi.mock('./database', () => ({
  getDatabase: () => mockDb,
}))

import { meetingRepository } from './meetingRepository'

describe('MeetingRepository', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('getMeetings', () => {
    it('delegates to database getMeetings', async () => {
      const fakeMeetings = [
        { meeting_id: '1', title: 'Meeting 1' },
        { meeting_id: '2', title: 'Meeting 2' },
      ]
      mockDb.getMeetings.mockResolvedValueOnce(fakeMeetings)

      const result = await meetingRepository.getMeetings()

      expect(mockDb.getMeetings).toHaveBeenCalledOnce()
      expect(result).toEqual(fakeMeetings)
    })
  })

  describe('getMeeting', () => {
    it('delegates to database getMeeting with the correct ID', async () => {
      const fakeMeeting = { meeting_id: 'abc', title: 'Test' }
      mockDb.getMeeting.mockResolvedValueOnce(fakeMeeting)

      const result = await meetingRepository.getMeeting('abc')

      expect(mockDb.getMeeting).toHaveBeenCalledWith('abc')
      expect(result).toEqual(fakeMeeting)
    })

    it('returns null when meeting not found', async () => {
      mockDb.getMeeting.mockResolvedValueOnce(null)

      const result = await meetingRepository.getMeeting('nonexistent')

      expect(result).toBeNull()
    })
  })

  describe('createMeeting', () => {
    it('creates a meeting in database and queues sync', async () => {
      const result = await meetingRepository.createMeeting('Team Standup', 3600)

      expect(result.meeting_id).toBe('mock-uuid-1234')
      expect(result.title).toBe('Team Standup')
      expect(result.status).toBe('recording')
      expect(result.sync_status).toBe('local_only')
      expect(result.version).toBe(1)
      expect(result.duration_seconds).toBe(3600)
      expect(result.created_at).toBeDefined()
      expect(result.updated_at).toBeDefined()

      expect(mockDb.insertMeeting).toHaveBeenCalledWith(
        expect.objectContaining({
          meeting_id: 'mock-uuid-1234',
          title: 'Team Standup',
          status: 'recording',
          sync_status: 'local_only',
        }),
      )

      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith(
        'create',
        'mock-uuid-1234',
        expect.objectContaining({
          title: 'Team Standup',
          duration_seconds: 3600,
        }),
      )
    })

    it('uses "Untitled Meeting" when title is empty', async () => {
      const result = await meetingRepository.createMeeting('')

      expect(result.title).toBe('Untitled Meeting')
      expect(mockDb.insertMeeting).toHaveBeenCalledWith(
        expect.objectContaining({ title: 'Untitled Meeting' }),
      )
    })

    it('handles undefined durationSeconds', async () => {
      const result = await meetingRepository.createMeeting('Quick Chat')

      expect(result.duration_seconds).toBeUndefined()
      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith(
        'create',
        'mock-uuid-1234',
        expect.objectContaining({
          title: 'Quick Chat',
          duration_seconds: undefined,
        }),
      )
    })
  })

  describe('updateMeeting', () => {
    it('updates meeting in database with pending_sync status and queues sync', async () => {
      await meetingRepository.updateMeeting('meeting-1', { title: 'Updated Title' })

      expect(mockDb.updateMeeting).toHaveBeenCalledWith(
        'meeting-1',
        expect.objectContaining({
          title: 'Updated Title',
          sync_status: 'pending_sync',
          updated_at: expect.any(String),
        }),
      )

      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith('update', 'meeting-1', {
        title: 'Updated Title',
      })
    })
  })

  describe('deleteMeeting', () => {
    it('deletes meeting from database and queues sync', async () => {
      await meetingRepository.deleteMeeting('meeting-1')

      expect(mockDb.deleteMeeting).toHaveBeenCalledWith('meeting-1')
      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith('delete', 'meeting-1')
    })
  })

  describe('queueAudioUpload', () => {
    it('updates meeting status and queues upload_audio sync', async () => {
      await meetingRepository.queueAudioUpload('meeting-1', '/path/to/audio.webm')

      expect(mockDb.updateMeeting).toHaveBeenCalledWith('meeting-1', {
        status: 'pending_upload',
        audio_file_path: '/path/to/audio.webm',
      })

      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith('upload_audio', 'meeting-1', {
        audio_file_path: '/path/to/audio.webm',
        provider: 'deepgram',
      })
    })
  })

  describe('requestSummary', () => {
    it('updates meeting status to summarizing and queues request_summary', async () => {
      await meetingRepository.requestSummary('meeting-1')

      expect(mockDb.updateMeeting).toHaveBeenCalledWith('meeting-1', {
        status: 'summarizing',
      })
      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith('request_summary', 'meeting-1', {
        provider: 'claude',
        model: undefined,
      })
    })

    it('uses custom provider and model', async () => {
      await meetingRepository.requestSummary('meeting-1', 'groq', 'llama-3')

      expect(mockDb.addToSyncQueue).toHaveBeenCalledWith('request_summary', 'meeting-1', {
        provider: 'groq',
        model: 'llama-3',
      })
    })
  })

  describe('getPendingCount', () => {
    it('delegates to database getPendingCount', async () => {
      mockDb.getPendingCount.mockResolvedValueOnce(5)

      const result = await meetingRepository.getPendingCount()

      expect(mockDb.getPendingCount).toHaveBeenCalledOnce()
      expect(result).toBe(5)
    })
  })
})

/**
 * Cloud transcription service — handles audio upload and status polling.
 */

import { authFetch, getAccessToken } from './authService'
import { TranscriptionStatus } from '@/types'
import { config } from './config'

/**
 * Upload audio file for transcription. Returns the transcription job ID.
 */
export async function uploadForTranscription(
  audioUri: string,
  meetingId: string,
  options?: { language?: string; provider?: string },
): Promise<string> {
  const token = await getAccessToken()
  if (!token) throw new Error('Not authenticated')

  // Fetch the audio file as a blob
  const audioResponse = await fetch(audioUri)
  const audioBlob = await audioResponse.blob()

  const formData = new FormData()
  formData.append('audio', audioBlob, 'recording.m4a')
  formData.append('meeting_id', meetingId)
  if (options?.provider) formData.append('provider', options.provider)
  if (options?.language) formData.append('language', options.language)

  const res = await fetch(`${config.apiUrl}/api/transcription/upload`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}` },
    body: formData,
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Upload failed' }))
    throw new Error(err.detail || `Upload failed: ${res.status}`)
  }

  const data = await res.json()
  return data.transcription_id
}

/**
 * Poll transcription status.
 */
export async function pollTranscriptionStatus(
  transcriptionId: string,
): Promise<TranscriptionStatus> {
  const res = await authFetch(`/api/transcription/${transcriptionId}/status`)
  if (!res.ok) throw new Error(`Status check failed: ${res.status}`)
  return res.json()
}

/**
 * Check remaining transcription quota.
 */
export async function getTranscriptionQuota(): Promise<{
  remaining_minutes: number
  plan_limit: number
  used_minutes: number
}> {
  const res = await authFetch('/api/transcription/quota')
  if (!res.ok) throw new Error('Quota check failed')
  return res.json()
}

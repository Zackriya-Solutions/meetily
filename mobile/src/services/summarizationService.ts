/**
 * Cloud summarization service — request and poll summary generation.
 */

import { authFetch } from './authService'
import { SummaryStatus } from '@/types'

/**
 * Start summarization for a meeting.
 */
export async function startSummarization(
  meetingId: string,
  options?: { provider?: string; model?: string; customPrompt?: string },
): Promise<string> {
  const res = await authFetch('/api/summarize', {
    method: 'POST',
    body: JSON.stringify({
      meeting_id: meetingId,
      provider: options?.provider || 'claude',
      model: options?.model,
      custom_prompt: options?.customPrompt,
    }),
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Summarization failed' }))
    throw new Error(err.detail || `Summarization failed: ${res.status}`)
  }

  const data = await res.json()
  return data.meeting_id
}

/**
 * Poll summary status for a meeting.
 */
export async function pollSummaryStatus(meetingId: string): Promise<SummaryStatus> {
  const res = await authFetch(`/api/summarize/${meetingId}/status`)
  if (!res.ok) throw new Error(`Summary status check failed: ${res.status}`)
  return res.json()
}

/**
 * Usage event service — buffers events and flushes to the cloud API.
 *
 * Events are tracked both in Rust (recording/transcription) and TypeScript
 * (meetings, summaries, sessions, active time). This service handles the
 * TypeScript-side events and coordinates flushing all events to the cloud.
 */

import { invoke } from '@tauri-apps/api/core'
import { getAccessToken, getAuthUserId } from './authService'

// Cloud API URL — same as authService
let cloudApiUrl: string = process.env.NEXT_PUBLIC_CLOUD_API_URL || ''

async function getBaseUrl(): Promise<string> {
  if (cloudApiUrl) return cloudApiUrl
  try {
    const res = await fetch('http://localhost:5167/api/config')
    if (res.ok) {
      const data = await res.json()
      if (data.cloud_api_url) {
        cloudApiUrl = data.cloud_api_url
        return cloudApiUrl
      }
    }
  } catch { /* ignore */ }
  return 'http://localhost:5167'
}

// ── Track events via Rust buffer ────────────────────────────────────

export async function trackEvent(
  eventType: string,
  value: number,
  metadata?: Record<string, any>,
): Promise<void> {
  try {
    await invoke('usage_track_event', {
      eventType,
      value,
      metadata: metadata ? JSON.parse(JSON.stringify(metadata)) : null,
    })
  } catch (e) {
    console.warn('[UsageService] Failed to track event:', e)
  }
}

// ── Convenience methods ─────────────────────────────────────────────

export async function trackMeetingCreated(meetingId?: string): Promise<void> {
  await trackEvent('meeting_created', 1, meetingId ? { meeting_id: meetingId } : undefined)
}

export async function trackSummaryGenerated(
  meetingId: string,
  llmProvider: string,
  llmModel: string,
): Promise<void> {
  await trackEvent('summary_generated', 1, {
    meeting_id: meetingId,
    llm_provider: llmProvider,
    llm_model: llmModel,
  })
}

export async function trackSessionStarted(): Promise<void> {
  await trackEvent('session_started', 1)
}

export async function trackSessionEnded(durationMinutes: number): Promise<void> {
  await trackEvent('session_ended', durationMinutes)
}

export async function trackActiveMinutes(minutes: number): Promise<void> {
  await trackEvent('active_minutes', minutes)
}

// ── Flush buffer to cloud API ───────────────────────────────────────

let flushInterval: ReturnType<typeof setInterval> | null = null

export async function flushEvents(): Promise<void> {
  try {
    const token = await getAccessToken()
    if (!token) return // Not authenticated — skip flush

    // Drain events from Rust buffer
    const events = await invoke<any[]>('usage_flush_events')
    if (!events || events.length === 0) return

    // Get device ID from cached value (set during init)
    const deviceId = (window as any).__iqcapture_device_id || 'unknown'

    // Get auth user_id if available (links usage events to authenticated user)
    let userId: string | null = null
    try {
      userId = await getAuthUserId()
    } catch { /* ignore */ }

    const baseUrl = await getBaseUrl()
    const res = await fetch(`${baseUrl}/api/usage/events`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        device_id: deviceId,
        user_id: userId,
        events,
      }),
    })

    if (res.ok) {
      const data = await res.json()
      console.log(`[UsageService] Flushed ${data.ingested} events to cloud`)
    } else {
      console.warn('[UsageService] Flush failed:', res.status)
      // Re-buffer events on failure (push them back)
      for (const event of events) {
        await invoke('usage_track_event', {
          eventType: event.event_type,
          value: event.value,
          metadata: event.metadata || null,
        })
      }
    }
  } catch (e) {
    console.warn('[UsageService] Flush error:', e)
  }
}

// ── Lifecycle ───────────────────────────────────────────────────────

export function startPeriodicFlush(intervalMs: number = 60_000): void {
  if (flushInterval) return
  flushInterval = setInterval(flushEvents, intervalMs)
}

export function stopPeriodicFlush(): void {
  if (flushInterval) {
    clearInterval(flushInterval)
    flushInterval = null
  }
}

/**
 * Initialize the usage service — start periodic flush and track session start.
 * Call once on app mount.
 */
export async function initUsageService(): Promise<void> {
  await trackSessionStarted()
  startPeriodicFlush()

  // Store device_id globally for flush — use Tauri store (same as Analytics.getPersistentUserId)
  try {
    const { Store } = await import('@tauri-apps/plugin-store')
    const store = await Store.load('analytics.json')
    const storedId = await store.get<string>('user_id')
    if (storedId) (window as any).__iqcapture_device_id = storedId
  } catch {
    // Fallback to localStorage
    try {
      const stored = localStorage.getItem('iqcapture_user_id')
      if (stored) (window as any).__iqcapture_device_id = stored
    } catch { /* ignore */ }
  }

  // Flush on app close
  window.addEventListener('beforeunload', () => {
    stopPeriodicFlush()
    // Persist buffer to disk via Rust (sync — best effort on exit)
    invoke('usage_persist_buffer').catch(() => {})
  })
}

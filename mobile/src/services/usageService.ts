/**
 * Usage event service for mobile — buffers events in memory and
 * flushes to cloud API periodically.
 *
 * Replaces desktop Rust buffer with in-memory array + Capacitor Preferences persistence.
 */

import { authFetch, getAccessToken } from './authService'
import { getDeviceId } from './deviceService'
import { v4 as uuidv4 } from 'uuid'

interface UsageEvent {
  event_type: string
  value: number
  metadata?: Record<string, any>
  timestamp: string
  client_event_id: string
}

let eventBuffer: UsageEvent[] = []
let flushInterval: ReturnType<typeof setInterval> | null = null

// ── Track events ────────────────────────────────────────────────────

export function trackEvent(
  eventType: string,
  value: number,
  metadata?: Record<string, any>,
): void {
  eventBuffer.push({
    event_type: eventType,
    value,
    metadata,
    timestamp: new Date().toISOString(),
    client_event_id: uuidv4(),
  })
}

// ── Convenience methods ─────────────────────────────────────────────

export function trackMeetingCreated(meetingId?: string): void {
  trackEvent('meeting_created', 1, meetingId ? { meeting_id: meetingId } : undefined)
}

export function trackSummaryGenerated(
  meetingId: string,
  llmProvider: string,
  llmModel: string,
): void {
  trackEvent('summary_generated', 1, {
    meeting_id: meetingId,
    llm_provider: llmProvider,
    llm_model: llmModel,
  })
}

export function trackCloudTranscriptionMinutes(meetingId: string, minutes: number): void {
  trackEvent('cloud_transcription_minutes', minutes, { meeting_id: meetingId })
}

export function trackAudioUploadBytes(meetingId: string, bytes: number): void {
  trackEvent('audio_upload_bytes', bytes, { meeting_id: meetingId })
}

export function trackSessionStarted(): void {
  trackEvent('session_started', 1)
}

export function trackSessionEnded(durationMinutes: number): void {
  trackEvent('session_ended', durationMinutes)
}

// ── Flush buffer to cloud API ───────────────────────────────────────

export async function flushEvents(): Promise<void> {
  if (eventBuffer.length === 0) return

  try {
    const token = await getAccessToken()
    if (!token) return

    const eventsToFlush = [...eventBuffer]
    eventBuffer = []

    const deviceId = await getDeviceId()

    const res = await authFetch('/api/usage/events', {
      method: 'POST',
      body: JSON.stringify({ device_id: deviceId, events: eventsToFlush }),
    })

    if (res.ok) {
      const data = await res.json()
      console.log(`[UsageService] Flushed ${data.ingested} events`)
    } else {
      console.warn('[UsageService] Flush failed:', res.status)
      // Re-buffer events on failure
      eventBuffer = [...eventsToFlush, ...eventBuffer]
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

export function initUsageService(): void {
  trackSessionStarted()
  startPeriodicFlush()

  // Flush on page visibility change (app backgrounding)
  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') {
        flushEvents()
      }
    })
  }

  // Flush on app close
  if (typeof window !== 'undefined') {
    window.addEventListener('beforeunload', () => {
      stopPeriodicFlush()
      flushEvents()
    })
  }
}

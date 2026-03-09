/**
 * Audio recording service using Web MediaRecorder API.
 *
 * Works on both native Capacitor (via WebView) and browser dev mode.
 * Records in webm/opus format, saves to Capacitor Filesystem on native
 * or returns a blob URL on web.
 */

import { Filesystem, Directory } from '@capacitor/filesystem'

let mediaRecorder: MediaRecorder | null = null
let audioChunks: Blob[] = []
let stream: MediaStream | null = null

function isNativePlatform(): boolean {
  if (typeof window === 'undefined') return false
  return !!(window as any).Capacitor?.isNativePlatform?.()
}

export async function requestMicrophonePermission(): Promise<boolean> {
  try {
    const s = await navigator.mediaDevices.getUserMedia({ audio: true })
    s.getTracks().forEach(t => t.stop())
    return true
  } catch {
    return false
  }
}

export async function startRecording(): Promise<void> {
  audioChunks = []

  stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
      sampleRate: 44100,
    },
  })

  // Prefer webm/opus, fall back to whatever the browser supports
  const mimeType = MediaRecorder.isTypeSupported('audio/webm;codecs=opus')
    ? 'audio/webm;codecs=opus'
    : MediaRecorder.isTypeSupported('audio/mp4')
      ? 'audio/mp4'
      : ''

  mediaRecorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined)

  mediaRecorder.ondataavailable = (event) => {
    if (event.data.size > 0) {
      audioChunks.push(event.data)
    }
  }

  // Collect data every second for robustness
  mediaRecorder.start(1000)
}

export function pauseRecording(): void {
  if (mediaRecorder?.state === 'recording') {
    mediaRecorder.pause()
  }
}

export function resumeRecording(): void {
  if (mediaRecorder?.state === 'paused') {
    mediaRecorder.resume()
  }
}

export async function stopRecording(meetingId: string): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!mediaRecorder) {
      reject(new Error('No recording in progress'))
      return
    }

    mediaRecorder.onstop = async () => {
      try {
        const mimeType = mediaRecorder?.mimeType || 'audio/webm'
        const blob = new Blob(audioChunks, { type: mimeType })
        audioChunks = []

        // Stop all tracks
        if (stream) {
          stream.getTracks().forEach(t => t.stop())
          stream = null
        }
        mediaRecorder = null

        const ext = mimeType.includes('mp4') ? 'm4a' : 'webm'
        const filename = `recording_${meetingId}.${ext}`

        if (isNativePlatform()) {
          // Save to Capacitor Filesystem
          const reader = new FileReader()
          reader.onloadend = async () => {
            try {
              const base64 = (reader.result as string).split(',')[1]
              const result = await Filesystem.writeFile({
                path: `recordings/${filename}`,
                data: base64,
                directory: Directory.Data,
                recursive: true,
              })
              resolve(result.uri)
            } catch (e) {
              reject(e)
            }
          }
          reader.onerror = reject
          reader.readAsDataURL(blob)
        } else {
          // Web fallback — return blob URL
          resolve(URL.createObjectURL(blob))
        }
      } catch (e) {
        reject(e)
      }
    }

    mediaRecorder.stop()
  })
}

export function isRecordingActive(): boolean {
  return mediaRecorder?.state === 'recording' || mediaRecorder?.state === 'paused'
}

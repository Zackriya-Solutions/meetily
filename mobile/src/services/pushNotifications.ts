/**
 * Push notification service using Capacitor Local Notifications.
 *
 * Used to notify users when background transcription or summarization
 * completes. Uses local notifications (no server push infrastructure required).
 *
 * For server-side push (FCM/APNS), add @capacitor/push-notifications
 * and register the device token with the backend.
 */

let localNotificationsModule: any = null
let notificationId = 1

async function getLocalNotifications(): Promise<any | null> {
  if (localNotificationsModule) return localNotificationsModule

  // Not available in browser
  if (typeof window !== 'undefined' && !(window as any).Capacitor?.isNativePlatform?.()) {
    return null
  }

  try {
    const mod = await import('@capacitor/local-notifications')
    localNotificationsModule = mod.LocalNotifications
    return localNotificationsModule
  } catch {
    return null
  }
}

export async function requestNotificationPermission(): Promise<boolean> {
  const ln = await getLocalNotifications()
  if (!ln) return false

  try {
    const result = await ln.requestPermissions()
    return result.display === 'granted'
  } catch {
    return false
  }
}

export async function notifyTranscriptionComplete(meetingTitle: string, meetingId: string): Promise<void> {
  const ln = await getLocalNotifications()
  if (!ln) {
    console.log(`[Notifications] Transcription complete: ${meetingTitle}`)
    return
  }

  try {
    await ln.schedule({
      notifications: [{
        id: notificationId++,
        title: 'Transcription Complete',
        body: `"${meetingTitle}" has been transcribed and is ready to view.`,
        extra: { meetingId, action: 'view_transcript' },
      }],
    })
  } catch (e) {
    console.warn('[Notifications] Failed to schedule:', e)
  }
}

export async function notifySummaryComplete(meetingTitle: string, meetingId: string): Promise<void> {
  const ln = await getLocalNotifications()
  if (!ln) {
    console.log(`[Notifications] Summary complete: ${meetingTitle}`)
    return
  }

  try {
    await ln.schedule({
      notifications: [{
        id: notificationId++,
        title: 'Summary Ready',
        body: `AI summary for "${meetingTitle}" is ready.`,
        extra: { meetingId, action: 'view_summary' },
      }],
    })
  } catch (e) {
    console.warn('[Notifications] Failed to schedule:', e)
  }
}

export async function initNotifications(): Promise<void> {
  const granted = await requestNotificationPermission()
  if (granted) {
    console.log('[Notifications] Permission granted')
  }
}

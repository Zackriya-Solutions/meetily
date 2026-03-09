/**
 * Deep linking handler for the mobile app.
 *
 * Handles URLs like:
 *   iqcapture://auth/reset-password?email=user@example.com&code=123456
 *   https://app.iqcapture.app/auth/reset-password?email=...&code=...
 *   https://app.iqcapture.app/auth/verify-email?email=...&code=...
 *
 * Requires @capacitor/app plugin (already in dependencies).
 */

import { App as CapacitorApp } from '@capacitor/app'

type DeepLinkHandler = (path: string, params: URLSearchParams) => void

let handler: DeepLinkHandler | null = null

/**
 * Register a handler that will be called when the app is opened via deep link.
 * Call this from the root layout component.
 */
export function registerDeepLinkHandler(onDeepLink: DeepLinkHandler): () => void {
  handler = onDeepLink

  // Listen for app URL open events (Capacitor)
  const listener = CapacitorApp.addListener('appUrlOpen', (event) => {
    try {
      const url = new URL(event.url)
      const path = url.pathname || url.host + url.pathname
      handler?.(path, url.searchParams)
    } catch {
      // Custom scheme like iqcapture://auth/reset-password
      try {
        const parts = event.url.replace(/^iqcapture:\/\//, '')
        const [pathPart, queryPart] = parts.split('?')
        const params = new URLSearchParams(queryPart || '')
        handler?.('/' + pathPart, params)
      } catch {
        console.warn('[DeepLink] Failed to parse URL:', event.url)
      }
    }
  })

  return () => {
    handler = null
    listener.then(l => l.remove())
  }
}

/**
 * Parse a deep link path and return the appropriate route for the Next.js router.
 *
 * Returns null if the link isn't recognized.
 */
export function parseDeepLink(path: string, params: URLSearchParams): string | null {
  const normalized = path.replace(/^\/+/, '/')

  if (normalized.startsWith('/auth/reset-password')) {
    const email = params.get('email')
    const code = params.get('code')
    if (email) {
      let route = `/auth/reset-password?email=${encodeURIComponent(email)}`
      if (code) route += `&code=${encodeURIComponent(code)}`
      return route
    }
  }

  if (normalized.startsWith('/auth/verify-email')) {
    const email = params.get('email')
    const code = params.get('code')
    if (email) {
      let route = `/auth/verify-email?email=${encodeURIComponent(email)}`
      if (code) route += `&code=${encodeURIComponent(code)}`
      return route
    }
  }

  if (normalized.startsWith('/meeting/')) {
    return normalized
  }

  return null
}

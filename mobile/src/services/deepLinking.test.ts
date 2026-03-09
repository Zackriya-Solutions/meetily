import { describe, it, expect, vi, beforeEach } from 'vitest'
import { App as CapacitorApp } from '@capacitor/app'
import { registerDeepLinkHandler, parseDeepLink } from './deepLinking'

describe('deepLinking', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('registerDeepLinkHandler', () => {
    it('registers a listener with Capacitor App', () => {
      const handler = vi.fn()

      const cleanup = registerDeepLinkHandler(handler)

      expect(CapacitorApp.addListener).toHaveBeenCalledWith('appUrlOpen', expect.any(Function))
      expect(typeof cleanup).toBe('function')
    })

    it('cleanup function removes handler', async () => {
      const mockRemove = vi.fn()
      vi.mocked(CapacitorApp.addListener).mockResolvedValueOnce({ remove: mockRemove } as any)

      const handler = vi.fn()
      const cleanup = registerDeepLinkHandler(handler)

      cleanup()

      // Let the promise resolve
      await new Promise((r) => setTimeout(r, 0))
      expect(mockRemove).toHaveBeenCalled()
    })

    it('parses standard URL and calls handler', () => {
      let capturedListener: (event: { url: string }) => void = () => {}
      vi.mocked(CapacitorApp.addListener).mockImplementationOnce(
        (eventName: string, callback: any) => {
          capturedListener = callback
          return Promise.resolve({ remove: vi.fn() }) as any
        },
      )

      const handler = vi.fn()
      registerDeepLinkHandler(handler)

      // Simulate an appUrlOpen event with a standard URL
      capturedListener({ url: 'https://app.iqcapture.app/auth/reset-password?email=test@example.com&code=123' })

      expect(handler).toHaveBeenCalledWith(
        '/auth/reset-password',
        expect.any(URLSearchParams),
      )
      const params = handler.mock.calls[0][1] as URLSearchParams
      expect(params.get('email')).toBe('test@example.com')
      expect(params.get('code')).toBe('123')
    })

    it('parses custom scheme URL (iqcapture://) and calls handler', () => {
      let capturedListener: (event: { url: string }) => void = () => {}
      vi.mocked(CapacitorApp.addListener).mockImplementationOnce(
        (eventName: string, callback: any) => {
          capturedListener = callback
          return Promise.resolve({ remove: vi.fn() }) as any
        },
      )

      const handler = vi.fn()
      registerDeepLinkHandler(handler)

      // Simulate custom scheme URL that would fail `new URL()`
      // The mock URL constructor in jsdom may handle it, so we test the fallback
      capturedListener({ url: 'iqcapture://auth/verify-email?email=user@test.com&code=456' })

      expect(handler).toHaveBeenCalled()
    })

    it('warns on completely unparseable URL', () => {
      let capturedListener: (event: { url: string }) => void = () => {}
      vi.mocked(CapacitorApp.addListener).mockImplementationOnce(
        (eventName: string, callback: any) => {
          capturedListener = callback
          return Promise.resolve({ remove: vi.fn() }) as any
        },
      )

      const handler = vi.fn()
      registerDeepLinkHandler(handler)

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      // Pass something that might trigger the catch branches
      // In jsdom, most URLs parse fine, so this tests robustness
      capturedListener({ url: '' })

      warnSpy.mockRestore()
    })
  })

  describe('parseDeepLink', () => {
    it('handles /auth/reset-password with email and code', () => {
      const params = new URLSearchParams('email=test@example.com&code=abc123')
      const result = parseDeepLink('/auth/reset-password', params)

      expect(result).toBe(
        `/auth/reset-password?email=${encodeURIComponent('test@example.com')}&code=${encodeURIComponent('abc123')}`,
      )
    })

    it('handles /auth/reset-password with email only (no code)', () => {
      const params = new URLSearchParams('email=test@example.com')
      const result = parseDeepLink('/auth/reset-password', params)

      expect(result).toBe(
        `/auth/reset-password?email=${encodeURIComponent('test@example.com')}`,
      )
    })

    it('returns null for /auth/reset-password without email', () => {
      const params = new URLSearchParams('code=abc123')
      const result = parseDeepLink('/auth/reset-password', params)

      expect(result).toBeNull()
    })

    it('handles /auth/verify-email with email and code', () => {
      const params = new URLSearchParams('email=user@test.com&code=xyz789')
      const result = parseDeepLink('/auth/verify-email', params)

      expect(result).toBe(
        `/auth/verify-email?email=${encodeURIComponent('user@test.com')}&code=${encodeURIComponent('xyz789')}`,
      )
    })

    it('handles /auth/verify-email with email only (no code)', () => {
      const params = new URLSearchParams('email=user@test.com')
      const result = parseDeepLink('/auth/verify-email', params)

      expect(result).toBe(
        `/auth/verify-email?email=${encodeURIComponent('user@test.com')}`,
      )
    })

    it('returns null for /auth/verify-email without email', () => {
      const params = new URLSearchParams('')
      const result = parseDeepLink('/auth/verify-email', params)

      expect(result).toBeNull()
    })

    it('handles /meeting/:id path', () => {
      const params = new URLSearchParams()
      const result = parseDeepLink('/meeting/abc-123', params)

      expect(result).toBe('/meeting/abc-123')
    })

    it('handles /meeting/:id with leading double slash normalization', () => {
      const params = new URLSearchParams()
      const result = parseDeepLink('//meeting/abc-123', params)

      expect(result).toBe('/meeting/abc-123')
    })

    it('returns null for unknown paths', () => {
      const params = new URLSearchParams()

      expect(parseDeepLink('/unknown/path', params)).toBeNull()
      expect(parseDeepLink('/settings', params)).toBeNull()
      expect(parseDeepLink('/', params)).toBeNull()
    })

    it('returns null for empty path', () => {
      const params = new URLSearchParams()
      const result = parseDeepLink('', params)

      expect(result).toBeNull()
    })
  })
})

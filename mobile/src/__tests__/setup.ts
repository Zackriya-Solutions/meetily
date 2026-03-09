import '@testing-library/jest-dom/vitest'
import { vi } from 'vitest'

// ── Mock localStorage ──
const localStorageMock = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, value: string) => { store[key] = value }),
    removeItem: vi.fn((key: string) => { delete store[key] }),
    clear: vi.fn(() => { store = {} }),
    get length() { return Object.keys(store).length },
    key: vi.fn((i: number) => Object.keys(store)[i] ?? null),
  }
})()
Object.defineProperty(window, 'localStorage', { value: localStorageMock })

// ── Mock navigator.onLine ──
Object.defineProperty(navigator, 'onLine', { value: true, writable: true })

// ── Mock navigator.mediaDevices ──
Object.defineProperty(navigator, 'mediaDevices', {
  value: {
    getUserMedia: vi.fn().mockResolvedValue({
      getTracks: () => [{ stop: vi.fn() }],
    }),
  },
  writable: true,
})

// ── Mock MediaRecorder ──
class MockMediaRecorder {
  state = 'inactive'
  ondataavailable: ((e: any) => void) | null = null
  onstop: (() => void) | null = null
  onerror: ((e: any) => void) | null = null
  mimeType = 'audio/webm;codecs=opus'

  static isTypeSupported(type: string) {
    return type === 'audio/webm;codecs=opus'
  }

  start(_timeslice?: number) {
    this.state = 'recording'
  }

  stop() {
    this.state = 'inactive'
    setTimeout(() => this.onstop?.(), 0)
  }

  pause() { this.state = 'paused' }
  resume() { this.state = 'recording' }
}
Object.defineProperty(window, 'MediaRecorder', { value: MockMediaRecorder })

// ── Mock Capacitor Preferences ──
vi.mock('@capacitor/preferences', () => {
  const store: Record<string, string> = {}
  return {
    Preferences: {
      get: vi.fn(({ key }: { key: string }) => Promise.resolve({ value: store[key] ?? null })),
      set: vi.fn(({ key, value }: { key: string; value: string }) => {
        store[key] = value
        return Promise.resolve()
      }),
      remove: vi.fn(({ key }: { key: string }) => {
        delete store[key]
        return Promise.resolve()
      }),
    },
  }
})

// ── Mock Capacitor Filesystem ──
vi.mock('@capacitor/filesystem', () => ({
  Filesystem: {
    writeFile: vi.fn().mockResolvedValue({ uri: 'file:///mock/recording.webm' }),
    readFile: vi.fn().mockResolvedValue({ data: '' }),
    deleteFile: vi.fn().mockResolvedValue(undefined),
  },
  Directory: { Data: 'DATA', Documents: 'DOCUMENTS' },
}))

// ── Mock Capacitor App ──
vi.mock('@capacitor/app', () => ({
  App: {
    addListener: vi.fn().mockResolvedValue({ remove: vi.fn() }),
  },
}))

// ── Mock Capacitor Network ──
vi.mock('@capacitor/network', () => ({
  Network: {
    getStatus: vi.fn().mockResolvedValue({ connected: true }),
    addListener: vi.fn().mockResolvedValue({ remove: vi.fn() }),
  },
}))

// ── Mock secure storage (not available in test) ──
// Return a module whose plugin methods throw, so dynamic imports resolve
// but the source code's try/catch treats the plugin as unavailable.
vi.mock('@capacitor-community/secure-storage', () => ({
  SecureStoragePlugin: {
    get: vi.fn().mockRejectedValue(new Error('Not available in test')),
    set: vi.fn().mockRejectedValue(new Error('Not available in test')),
    remove: vi.fn().mockRejectedValue(new Error('Not available in test')),
  },
}))

// ── Mock biometric plugin (not available in test) ──
// Return module without BiometricAuth so plugin is undefined (falsy),
// causing source code to treat it as unavailable (passthrough).
vi.mock('@aparajita/capacitor-biometric-auth', () => ({}))

// ── Mock Capacitor SQLite (not available in test) ──
vi.mock('@capacitor-community/sqlite', () => ({
  CapacitorSQLite: {
    createConnection: vi.fn().mockRejectedValue(new Error('Not available in test')),
  },
}))

// ── Mock local notifications ──
vi.mock('@capacitor/local-notifications', () => ({
  LocalNotifications: {
    schedule: vi.fn().mockRejectedValue(new Error('Not available in test')),
  },
}))

// ── Mock Next.js navigation ──
vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(),
}))

// ── Mock next/font ──
vi.mock('next/font/google', () => ({
  Source_Sans_3: () => ({ variable: '--font-source-sans-3' }),
}))

// ── Global fetch mock ──
global.fetch = vi.fn().mockResolvedValue({
  ok: true,
  json: () => Promise.resolve({}),
  text: () => Promise.resolve(''),
  status: 200,
})

// Reset mocks between tests
beforeEach(() => {
  vi.clearAllMocks()
  localStorageMock.clear()
})

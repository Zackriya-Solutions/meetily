/**
 * Global test setup for Vitest
 * Mocks Tauri APIs, browser APIs, and Next.js modules
 */
import { vi, beforeEach } from 'vitest';
import '@testing-library/jest-dom/vitest';

// ── Mock @tauri-apps/api/core ───────────────────────────────────────
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockRejectedValue(new Error('invoke not mocked for this command')),
}));

// ── Mock @tauri-apps/api/event ──────────────────────────────────────
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
}));

// ── Mock @tauri-apps/api/app ────────────────────────────────────────
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.2.3'),
}));

// ── Mock @tauri-apps/plugin-store ───────────────────────────────────
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: vi.fn().mockImplementation(() => ({
    get: vi.fn().mockResolvedValue(null),
    set: vi.fn().mockResolvedValue(undefined),
    delete: vi.fn().mockResolvedValue(undefined),
    save: vi.fn().mockResolvedValue(undefined),
  })),
}));

// ── Mock @tauri-apps/plugin-fs ──────────────────────────────────────
vi.mock('@tauri-apps/plugin-fs', () => ({
  readTextFile: vi.fn().mockResolvedValue(''),
  writeTextFile: vi.fn().mockResolvedValue(undefined),
  exists: vi.fn().mockResolvedValue(false),
  mkdir: vi.fn().mockResolvedValue(undefined),
  readDir: vi.fn().mockResolvedValue([]),
}));

// ── Mock @tauri-apps/plugin-dialog ──────────────────────────────────
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
  message: vi.fn().mockResolvedValue(undefined),
  ask: vi.fn().mockResolvedValue(false),
  confirm: vi.fn().mockResolvedValue(false),
}));

// ── Mock @tauri-apps/plugin-os ──────────────────────────────────────
vi.mock('@tauri-apps/plugin-os', () => ({
  platform: vi.fn().mockResolvedValue('windows'),
  arch: vi.fn().mockResolvedValue('x86_64'),
  type: vi.fn().mockResolvedValue('Windows_NT'),
}));

// ── Mock @tauri-apps/plugin-clipboard-manager ───────────────────────
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
  readText: vi.fn().mockResolvedValue(''),
}));

// ── Mock next/navigation ────────────────────────────────────────────
vi.mock('next/navigation', () => ({
  useRouter: vi.fn().mockReturnValue({
    push: vi.fn(),
    back: vi.fn(),
    replace: vi.fn(),
    prefetch: vi.fn(),
    refresh: vi.fn(),
  }),
  useSearchParams: vi.fn().mockReturnValue(new URLSearchParams()),
  usePathname: vi.fn().mockReturnValue('/'),
}));

// ── Mock fetch globally ─────────────────────────────────────────────
global.fetch = vi.fn().mockResolvedValue({
  ok: true,
  json: vi.fn().mockResolvedValue({}),
  text: vi.fn().mockResolvedValue(''),
  status: 200,
});

// ── Mock localStorage ───────────────────────────────────────────────
const localStorageStore: Record<string, string> = {};
Object.defineProperty(global, 'localStorage', {
  value: {
    getItem: vi.fn((key: string) => localStorageStore[key] ?? null),
    setItem: vi.fn((key: string, value: string) => { localStorageStore[key] = value; }),
    removeItem: vi.fn((key: string) => { delete localStorageStore[key]; }),
    clear: vi.fn(() => { Object.keys(localStorageStore).forEach(k => delete localStorageStore[k]); }),
    length: 0,
    key: vi.fn(() => null),
  },
  writable: true,
});

// ── Mock window.addEventListener ────────────────────────────────────
const originalAddEventListener = window.addEventListener;
window.addEventListener = vi.fn(originalAddEventListener);

// ── Mock IndexedDB (minimal) ────────────────────────────────────────
const mockIDBRequest = (result: any) => ({
  result,
  error: null,
  onsuccess: null as any,
  onerror: null as any,
  onupgradeneeded: null as any,
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
});

const mockObjectStore = {
  put: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest(undefined);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  get: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest(null);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  getAll: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest([]);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  delete: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest(undefined);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  add: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest(undefined);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  count: vi.fn().mockImplementation(() => {
    const req = mockIDBRequest(0);
    setTimeout(() => req.onsuccess?.(), 0);
    return req;
  }),
  index: vi.fn().mockReturnValue({
    getAll: vi.fn().mockImplementation(() => {
      const req = mockIDBRequest([]);
      setTimeout(() => req.onsuccess?.(), 0);
      return req;
    }),
    count: vi.fn().mockImplementation(() => {
      const req = mockIDBRequest(0);
      setTimeout(() => req.onsuccess?.(), 0);
      return req;
    }),
    openCursor: vi.fn().mockImplementation(() => {
      const req = mockIDBRequest(null);
      setTimeout(() => req.onsuccess?.(), 0);
      return req;
    }),
  }),
  createIndex: vi.fn(),
};

const mockDB = {
  transaction: vi.fn().mockReturnValue({
    objectStore: vi.fn().mockReturnValue(mockObjectStore),
  }),
  objectStoreNames: { contains: vi.fn().mockReturnValue(false) },
  createObjectStore: vi.fn().mockReturnValue(mockObjectStore),
};

Object.defineProperty(global, 'indexedDB', {
  value: {
    open: vi.fn().mockImplementation(() => {
      const req = mockIDBRequest(mockDB);
      setTimeout(() => {
        req.onupgradeneeded?.({ target: req } as any);
        req.onsuccess?.();
      }, 0);
      return req;
    }),
    deleteDatabase: vi.fn(),
  },
  writable: true,
});

// ── Reset mocks between tests ───────────────────────────────────────
beforeEach(() => {
  vi.clearAllMocks();
  Object.keys(localStorageStore).forEach(k => delete localStorageStore[k]);
});

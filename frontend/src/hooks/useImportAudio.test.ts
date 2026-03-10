import { renderHook, act, waitFor } from '@testing-library/react';
import { vi, describe, it, expect, beforeEach, type Mock } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// Mock analytics
vi.mock('@/lib/analytics', () => ({
  default: {
    track: vi.fn().mockResolvedValue(undefined),
    trackError: vi.fn().mockResolvedValue(undefined),
  },
}));

import { useImportAudio } from './useImportAudio';

describe('useImportAudio', () => {
  const invokeMock = invoke as Mock;
  const listenMock = listen as Mock;

  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(vi.fn()); // unlisten function
  });

  // ── Initial State ──────────────────────────────────────────────────

  it('should initialize with idle status', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.status).toBe('idle');
  });

  it('should initialize with null progress', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.progress).toBeNull();
  });

  it('should initialize with null fileInfo', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.fileInfo).toBeNull();
  });

  it('should initialize with null error', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.error).toBeNull();
  });

  it('should initialize with isProcessing false', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.isProcessing).toBe(false);
  });

  it('should initialize with isBusy false', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(result.current.isBusy).toBe(false);
  });

  it('should expose selectFile function', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(typeof result.current.selectFile).toBe('function');
  });

  it('should expose startImport function', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(typeof result.current.startImport).toBe('function');
  });

  it('should expose cancelImport function', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(typeof result.current.cancelImport).toBe('function');
  });

  it('should expose reset function', () => {
    const { result } = renderHook(() => useImportAudio());
    expect(typeof result.current.reset).toBe('function');
  });

  // ── Event Listeners ─────────────────────────────────────────────────

  it('should register event listeners on mount', async () => {
    renderHook(() => useImportAudio());

    await waitFor(() => {
      expect(listenMock).toHaveBeenCalledWith('import-progress', expect.any(Function));
      expect(listenMock).toHaveBeenCalledWith('import-complete', expect.any(Function));
      expect(listenMock).toHaveBeenCalledWith('import-error', expect.any(Function));
    });
  });

  it('should clean up listeners on unmount', async () => {
    const unlistenFn = vi.fn();
    listenMock.mockResolvedValue(unlistenFn);

    const { unmount } = renderHook(() => useImportAudio());

    await waitFor(() => {
      expect(listenMock).toHaveBeenCalled();
    });

    unmount();
    expect(unlistenFn).toHaveBeenCalled();
  });

  // ── selectFile ──────────────────────────────────────────────────────

  it('should return file info on successful selection', async () => {
    const fileInfo = {
      path: '/test/audio.mp3',
      filename: 'audio.mp3',
      duration_seconds: 120,
      size_bytes: 1024000,
      format: 'mp3',
    };
    invokeMock.mockResolvedValue(fileInfo);

    const { result } = renderHook(() => useImportAudio());

    let selected: any;
    await act(async () => {
      selected = await result.current.selectFile();
    });

    expect(invokeMock).toHaveBeenCalledWith('select_and_validate_audio_command');
    expect(selected).toEqual(fileInfo);
    expect(result.current.fileInfo).toEqual(fileInfo);
    expect(result.current.status).toBe('idle');
  });

  it('should return null when user cancels file selection', async () => {
    invokeMock.mockResolvedValue(null);

    const { result } = renderHook(() => useImportAudio());

    let selected: any;
    await act(async () => {
      selected = await result.current.selectFile();
    });

    expect(selected).toBeNull();
    expect(result.current.status).toBe('idle');
  });

  it('should set error on selection failure', async () => {
    invokeMock.mockRejectedValue(new Error('Permission denied'));

    const { result } = renderHook(() => useImportAudio());

    await act(async () => {
      await result.current.selectFile();
    });

    expect(result.current.status).toBe('error');
    expect(result.current.error).toBe('Permission denied');
  });

  // ── validateFile ────────────────────────────────────────────────────

  it('should validate a file by path', async () => {
    const fileInfo = {
      path: '/test/audio.wav',
      filename: 'audio.wav',
      duration_seconds: 60,
      size_bytes: 512000,
      format: 'wav',
    };
    invokeMock.mockResolvedValue(fileInfo);

    const { result } = renderHook(() => useImportAudio());

    let validated: any;
    await act(async () => {
      validated = await result.current.validateFile('/test/audio.wav');
    });

    expect(invokeMock).toHaveBeenCalledWith('validate_audio_file_command', { path: '/test/audio.wav' });
    expect(validated).toEqual(fileInfo);
  });

  // ── startImport ─────────────────────────────────────────────────────

  it('should call start_import_audio_command', async () => {
    invokeMock.mockResolvedValue(undefined);

    const { result } = renderHook(() => useImportAudio());

    await act(async () => {
      await result.current.startImport('/test/audio.mp3', 'My Meeting', 'en', 'large-v3', 'localWhisper');
    });

    expect(invokeMock).toHaveBeenCalledWith('start_import_audio_command', {
      sourcePath: '/test/audio.mp3',
      title: 'My Meeting',
      language: 'en',
      model: 'large-v3',
      provider: 'localWhisper',
    });
    expect(result.current.status).toBe('processing');
  });

  it('should set error when startImport fails', async () => {
    invokeMock.mockRejectedValue(new Error('Import failed'));

    const { result } = renderHook(() => useImportAudio());

    await act(async () => {
      await result.current.startImport('/test/audio.mp3', 'Meeting');
    });

    expect(result.current.status).toBe('error');
    expect(result.current.error).toBe('Import failed');
  });

  // ── cancelImport ────────────────────────────────────────────────────

  it('should call cancel_import_command', async () => {
    invokeMock.mockResolvedValue(undefined);

    const { result } = renderHook(() => useImportAudio());

    await act(async () => {
      await result.current.cancelImport();
    });

    expect(invokeMock).toHaveBeenCalledWith('cancel_import_command');
    expect(result.current.status).toBe('idle');
  });

  // ── reset ───────────────────────────────────────────────────────────

  it('should reset all state', async () => {
    invokeMock.mockRejectedValue(new Error('fail'));

    const { result } = renderHook(() => useImportAudio());

    // Trigger an error state first
    await act(async () => {
      await result.current.selectFile();
    });
    expect(result.current.status).toBe('error');

    act(() => {
      result.current.reset();
    });

    expect(result.current.status).toBe('idle');
    expect(result.current.fileInfo).toBeNull();
    expect(result.current.progress).toBeNull();
    expect(result.current.error).toBeNull();
  });
});

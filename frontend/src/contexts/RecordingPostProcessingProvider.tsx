"use client";

import React, { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import type { RecordingCompletion } from '@/types';
import { invoke } from '@tauri-apps/api/core';
import { readPinnedSummaryLanguageDefault } from '@/lib/summary-language-preferences';

const ignoreStateUpdate = () => {};

/** Native finalization works even without this presentation layer. */
export function RecordingPostProcessingProvider({ children }: { children: React.ReactNode }) {
  const { handleRecordingCompletion } = useRecordingStop(ignoreStateUpdate, ignoreStateUpdate);
  const callback = useRef(handleRecordingCompletion);
  callback.current = handleRecordingCompletion;
  useEffect(() => {
    void invoke('set_native_summary_language_default', { language: readPinnedSummaryLanguageDefault() })
      .catch(error => console.warn('Native summary language preference migration deferred:', error));
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<RecordingCompletion>('meeting-saved', ({ payload }) => {
      void callback.current(payload).catch(error => console.error('Saved meeting refresh failed:', error));
    }).then(fn => { if (disposed) fn(); else unlisten = fn; })
      .catch(error => console.error('Native meeting completion listener failed:', error));
    return () => { disposed = true; unlisten?.(); };
  }, []);
  return <>{children}</>;
}

import { useCallback, useEffect, useState } from 'react';

const MRU_KEY = 'summaryLanguageRecents';
const PINNED_KEY = 'summaryLanguageDefault';
const MAX_RECENTS = 5;

function readPinnedFromStorage(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(PINNED_KEY);
  } catch {
    return null;
  }
}

function writePinnedToStorage(value: string | null): void {
  if (typeof window === 'undefined') return;
  try {
    if (value) window.localStorage.setItem(PINNED_KEY, value);
    else window.localStorage.removeItem(PINNED_KEY);
  } catch {
    // Silent — preference is cosmetic; resolution falls back to transcription.
  }
}

function readFromStorage(): string[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(MRU_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is string => typeof x === 'string' && x.length > 0)
      .slice(0, MAX_RECENTS);
  } catch {
    return [];
  }
}

function writeToStorage(values: string[]): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(MRU_KEY, JSON.stringify(values));
  } catch {
    // Quota exceeded / incognito — cosmetic list only, silent.
  }
}

/**
 * MRU list of recently used summary languages (max 5, localStorage).
 * Shared by SummaryLanguageSettings (chips) and LanguagePickerPopover (recents).
 *
 * addRecent: push to front, dedupe, trim to MAX_RECENTS, persist.
 */
export function useRecentLanguages() {
  const [recents, setRecents] = useState<string[]>(() => readFromStorage());
  const [pinned, setPinnedState] = useState<string | null>(() => readPinnedFromStorage());

  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === MRU_KEY) setRecents(readFromStorage());
      if (e.key === PINNED_KEY) setPinnedState(readPinnedFromStorage());
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  const addRecent = useCallback((code: string) => {
    const trimmed = code.trim();
    if (!trimmed) return;
    setRecents((prev) => {
      const deduped = [trimmed, ...prev.filter((c) => c !== trimmed)].slice(0, MAX_RECENTS);
      writeToStorage(deduped);
      return deduped;
    });
  }, []);

  const removeRecent = useCallback((code: string) => {
    setRecents((prev) => {
      const updated = prev.filter((c) => c !== code);
      writeToStorage(updated);
      return updated;
    });
    setPinnedState((prev) => {
      if (prev !== code) return prev;
      writePinnedToStorage(null);
      return null;
    });
  }, []);

  const setPinned = useCallback((code: string | null) => {
    setPinnedState(code);
    writePinnedToStorage(code);
  }, []);

  return { recents, pinned, addRecent, removeRecent, setPinned };
}

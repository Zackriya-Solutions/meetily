'use client';

import { createContext, useContext, useState, useCallback, useEffect, useMemo, ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { NameSuggestion, ResolvedSpeaker, SpeakerProfile } from '@/types';

export const PRESET_COLORS = [
  '#6366f1', '#f59e0b', '#10b981', '#ef4444', '#3b82f6',
  '#8b5cf6', '#ec4899', '#14b8a6', '#f97316', '#84cc16',
];

export function pickNextColor(usedColors: string[]): string {
  const unused = PRESET_COLORS.find(c => !usedColors.includes(c));
  return unused ?? PRESET_COLORS[usedColors.length % PRESET_COLORS.length];
}

interface SpeakerContextType {
  profiles: SpeakerProfile[];
  resolvedSpeakers: ResolvedSpeaker[];
  speakerMap: Record<string, { display_name: string; color: string }>;
  suggestions: NameSuggestion[];
  loading: boolean;
  meetingId: string | null;
  setMeetingId: (id: string | null) => void;
  reload: () => Promise<void>;
  renameSpeaker: (speakerId: string, name: string, color?: string, isSelf?: boolean) => Promise<void>;
  detectNames: () => Promise<NameSuggestion[]>;
  applySuggestion: (suggestion: NameSuggestion) => Promise<void>;
  dismissSuggestion: (suggestion: NameSuggestion) => void;
}

const SpeakerContext = createContext<SpeakerContextType | undefined>(undefined);

export function SpeakerProvider({ children }: { children: ReactNode }) {
  const [meetingId, setMeetingId] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<SpeakerProfile[]>([]);
  const [resolvedSpeakers, setResolvedSpeakers] = useState<ResolvedSpeaker[]>([]);
  const [suggestions, setSuggestions] = useState<NameSuggestion[]>([]);
  const [loading, setLoading] = useState(false);

  const speakerMap = useMemo<Record<string, { display_name: string; color: string }>>(
    () => Object.fromEntries(resolvedSpeakers.map(s => [s.speaker_id, { display_name: s.display_name, color: s.color }])),
    [resolvedSpeakers]
  );

  const loadProfiles = useCallback(async () => {
    try {
      const p = await invoke<SpeakerProfile[]>('get_speaker_profiles');
      setProfiles(p);
    } catch (e) {
      console.error('Failed to load speaker profiles', e);
    }
  }, []);

  const loadResolved = useCallback(async (id: string | null) => {
    if (!id) { setResolvedSpeakers([]); return; }
    try {
      const s = await invoke<ResolvedSpeaker[]>('get_meeting_speakers', { meetingId: id });
      setResolvedSpeakers(s);
    } catch (e) {
      console.error('Failed to load meeting speakers', e);
    }
  }, []);

  const reload = useCallback(async () => {
    setLoading(true);
    await Promise.all([loadProfiles(), loadResolved(meetingId)]);
    setLoading(false);
  }, [loadProfiles, loadResolved, meetingId]);

  // Reload whenever meetingId changes
  useEffect(() => {
    reload();
  }, [meetingId]); // eslint-disable-line react-hooks/exhaustive-deps

  const renameSpeaker = useCallback(async (
    speakerId: string,
    name: string,
    color?: string,
    isSelf?: boolean,
  ) => {
    if (!meetingId) return;
    const usedColors = profiles.map(p => p.color);
    const chosenColor = color ?? pickNextColor(usedColors);
    const isSelfVal = isSelf ?? speakerId === 'mic';

    try {
      const existing = resolvedSpeakers.find(s => s.speaker_id === speakerId);
      let profileId = existing?.profile_id ?? null;

      if (profileId) {
        await invoke('update_speaker_profile', {
          id: profileId,
          name,
          color: chosenColor,
          isSelf: isSelfVal,
          globalAutoApply: isSelfVal,
        });
      } else {
        const profile = await invoke<SpeakerProfile>('create_speaker_profile', {
          name,
          color: chosenColor,
          isSelf: isSelfVal,
          globalAutoApply: isSelfVal,
        });
        profileId = profile.id;
      }

      await invoke('set_speaker_mapping', { meetingId, speakerId, profileId });

      // Reload both profiles and resolved speakers so all consumers update
      await Promise.all([loadProfiles(), loadResolved(meetingId)]);

      toast.success(`Speaker named "${name}"`);
    } catch (e) {
      toast.error('Failed to save speaker name');
      console.error(e);
    }
  }, [meetingId, profiles, resolvedSpeakers, loadProfiles, loadResolved]);

  const detectNames = useCallback(async (): Promise<NameSuggestion[]> => {
    if (!meetingId) return [];
    try {
      const s = await invoke<NameSuggestion[]>('detect_speaker_names', { meetingId });
      setSuggestions(s);
      return s;
    } catch (e) {
      console.error('Name detection failed', e);
      return [];
    }
  }, [meetingId]);

  const applySuggestion = useCallback(async (suggestion: NameSuggestion) => {
    await renameSpeaker(suggestion.speaker_id, suggestion.name);
    setSuggestions(prev => prev.filter(s => s.speaker_id !== suggestion.speaker_id || s.name !== suggestion.name));
  }, [renameSpeaker]);

  const dismissSuggestion = useCallback((suggestion: NameSuggestion) => {
    setSuggestions(prev => prev.filter(s => s.speaker_id !== suggestion.speaker_id || s.name !== suggestion.name));
  }, []);

  return (
    <SpeakerContext.Provider value={{
      profiles, resolvedSpeakers, speakerMap, suggestions, loading,
      meetingId, setMeetingId,
      reload, renameSpeaker, detectNames, applySuggestion, dismissSuggestion,
    }}>
      {children}
    </SpeakerContext.Provider>
  );
}

export function useSpeakers() {
  const ctx = useContext(SpeakerContext);
  if (!ctx) throw new Error('useSpeakers must be used within SpeakerProvider');
  return ctx;
}


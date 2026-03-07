'use client';

import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { NameSuggestion, ResolvedSpeaker, SpeakerProfile } from '@/types';

const PRESET_COLORS = [
  '#6366f1', '#f59e0b', '#10b981', '#ef4444', '#3b82f6',
  '#8b5cf6', '#ec4899', '#14b8a6', '#f97316', '#84cc16',
];

/** Returns a color not currently used by any profile, or cycles through presets. */
export function pickNextColor(usedColors: string[]): string {
  const unused = PRESET_COLORS.find(c => !usedColors.includes(c));
  return unused ?? PRESET_COLORS[usedColors.length % PRESET_COLORS.length];
}

interface UseSpeakerProfilesOptions {
  meetingId?: string;
  /** Called after any speaker mapping change (e.g. to refresh transcript display) */
  onUpdated?: () => void;
}

export function useSpeakerProfiles({ meetingId, onUpdated }: UseSpeakerProfilesOptions = {}) {
  const [profiles, setProfiles] = useState<SpeakerProfile[]>([]);
  const [resolvedSpeakers, setResolvedSpeakers] = useState<ResolvedSpeaker[]>([]);
  const [suggestions, setSuggestions] = useState<NameSuggestion[]>([]);
  const [loading, setLoading] = useState(false);

  /** speakerMap suitable for passing to VirtualizedTranscriptView */
  const speakerMap: Record<string, { display_name: string; color: string }> =
    Object.fromEntries(resolvedSpeakers.map(s => [s.speaker_id, { display_name: s.display_name, color: s.color }]));

  const loadProfiles = useCallback(async () => {
    try {
      const p = await invoke<SpeakerProfile[]>('get_speaker_profiles');
      setProfiles(p);
    } catch (e) {
      console.error('Failed to load speaker profiles', e);
    }
  }, []);

  const loadResolved = useCallback(async () => {
    if (!meetingId) return;
    try {
      const s = await invoke<ResolvedSpeaker[]>('get_meeting_speakers', { meetingId });
      setResolvedSpeakers(s);
    } catch (e) {
      console.error('Failed to load meeting speakers', e);
    }
  }, [meetingId]);

  const reload = useCallback(async () => {
    setLoading(true);
    await Promise.all([loadProfiles(), loadResolved()]);
    setLoading(false);
  }, [loadProfiles, loadResolved]);

  useEffect(() => { reload(); }, [reload]);

  /** Detect names from transcript text and store suggestions */
  const detectNames = useCallback(async () => {
    if (!meetingId) return;
    try {
      const s = await invoke<NameSuggestion[]>('detect_speaker_names', { meetingId });
      setSuggestions(s);
      return s;
    } catch (e) {
      console.error('Name detection failed', e);
      return [];
    }
  }, [meetingId]);

  /**
   * Rename a speaker (speaker_id like 'mic', 'speaker_0', etc.) for this meeting.
   * If isSelf=true, marks the profile as the user's own identity for future meetings.
   */
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
      // Find existing profile with this name, or the one already mapped
      const existing = resolvedSpeakers.find(s => s.speaker_id === speakerId);
      let profileId = existing?.profile_id ?? null;

      if (profileId) {
        // Update existing profile
        await invoke('update_speaker_profile', {
          id: profileId,
          name,
          color: chosenColor,
          isSelf: isSelfVal,
          globalAutoApply: isSelfVal,
        });
        setProfiles(prev => prev.map(p => p.id === profileId ? { ...p, name, color: chosenColor, is_self: isSelfVal, global_auto_apply: isSelfVal } : p));
      } else {
        // Create a new profile
        const profile = await invoke<SpeakerProfile>('create_speaker_profile', {
          name,
          color: chosenColor,
          isSelf: isSelfVal,
          globalAutoApply: isSelfVal,
        });
        profileId = profile.id;
        setProfiles(prev => [...prev, profile]);
      }

      // Set the mapping for this meeting
      await invoke('set_speaker_mapping', { meetingId, speakerId, profileId });
      await loadResolved();
      onUpdated?.();
      toast.success(`Speaker named "${name}"`);
    } catch (e) {
      toast.error('Failed to save speaker name');
      console.error(e);
    }
  }, [meetingId, profiles, resolvedSpeakers, loadResolved, onUpdated]);

  /** Apply a name suggestion from auto-detection */
  const applySuggestion = useCallback(async (suggestion: NameSuggestion) => {
    await renameSpeaker(suggestion.speaker_id, suggestion.name);
    setSuggestions(prev => prev.filter(s => s.speaker_id !== suggestion.speaker_id));
  }, [renameSpeaker]);

  return {
    profiles,
    resolvedSpeakers,
    speakerMap,
    suggestions,
    loading,
    reload,
    renameSpeaker,
    detectNames,
    applySuggestion,
    PRESET_COLORS,
  };
}


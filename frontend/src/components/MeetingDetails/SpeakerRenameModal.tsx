"use client";

import React, { useState, useRef, useEffect, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Users, Volume2, Play, Square, Loader2, Check, Sparkles } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { getSpeakerColorClass } from '../TranscriptView';

interface SpeakerRenameModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  meetingId: string;
  speakers: string[];
  onComplete?: () => Promise<void> | void;
}

export function SpeakerRenameModal({
  open,
  onOpenChange,
  meetingId,
  speakers,
  onComplete,
}: SpeakerRenameModalProps) {
  // Deduplicate and filter valid speakers
  const uniqueSpeakers = Array.from(new Set(speakers.filter(s => s && s.trim().length > 0)));

  // State
  const [renames, setRenames] = useState<Record<string, string>>({});
  const [audioSamples, setAudioSamples] = useState<Record<string, string>>({});
  const [loadingAudio, setLoadingAudio] = useState<Record<string, boolean>>({});
  const [playingSpeaker, setPlayingSpeaker] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const audioRef = useRef<HTMLAudioElement | null>(null);

  // Stop any active audio playback
  const stopAudio = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current.currentTime = 0;
      audioRef.current = null;
    }
    setPlayingSpeaker(null);
  }, []);

  // Stop audio when modal closes or unmounts
  useEffect(() => {
    if (!open) {
      stopAudio();
    }
  }, [open, stopAudio]);

  useEffect(() => {
    return () => {
      stopAudio();
    };
  }, [stopAudio]);

  // Handle play/pause sample audio for a speaker
  const handleTogglePlaySample = async (speaker: string) => {
    // If this speaker is already playing, stop it
    if (playingSpeaker === speaker) {
      stopAudio();
      return;
    }

    // Stop whatever else might be playing
    stopAudio();

    try {
      let dataUrl = audioSamples[speaker];

      // Fetch sample if not already cached
      if (!dataUrl) {
        setLoadingAudio(prev => ({ ...prev, [speaker]: true }));
        try {
          dataUrl = await invoke<string>('api_get_speaker_sample_audio', {
            meetingId,
            speaker,
          });
          setAudioSamples(prev => ({ ...prev, [speaker]: dataUrl }));
        } catch (fetchErr: any) {
          console.error(`Failed to get sample audio for ${speaker}:`, fetchErr);
          toast.error(`Unable to load voice clip for ${speaker}`);
          setLoadingAudio(prev => ({ ...prev, [speaker]: false }));
          return;
        } finally {
          setLoadingAudio(prev => ({ ...prev, [speaker]: false }));
        }
      }

      if (!dataUrl) return;

      const audio = new Audio(dataUrl);
      audioRef.current = audio;

      audio.onended = () => {
        setPlayingSpeaker(null);
        audioRef.current = null;
      };

      audio.onerror = (e) => {
        console.error('Audio playback error:', e);
        toast.error('Voice clip playback failed');
        setPlayingSpeaker(null);
        audioRef.current = null;
      };

      await audio.play();
      setPlayingSpeaker(speaker);
    } catch (err: any) {
      console.error('Audio play error:', err);
      toast.error('Could not play audio sample');
      setPlayingSpeaker(null);
    }
  };

  const handleNameChange = (speaker: string, val: string) => {
    setRenames(prev => ({
      ...prev,
      [speaker]: val,
    }));
  };

  const handleSave = async () => {
    const changes: Record<string, string> = {};
    for (const spk of uniqueSpeakers) {
      const newName = renames[spk]?.trim();
      if (newName && newName !== spk) {
        changes[spk] = newName;
      }
    }

    if (Object.keys(changes).length === 0) {
      // Nothing changed, close modal
      onOpenChange(false);
      return;
    }

    setIsSaving(true);
    try {
      await invoke('api_rename_meeting_speakers', {
        meetingId,
        renames: changes,
      });

      toast.success('Speaker names updated successfully');
      if (onComplete) {
        await onComplete();
      }
      onOpenChange(false);
    } catch (err: any) {
      console.error('Failed to rename speakers:', err);
      toast.error(err?.toString() || 'Failed to update speaker names');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Dialog open={open && uniqueSpeakers.length > 0} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[540px] max-h-[90vh] flex flex-col p-6 overflow-hidden">
        <DialogHeader className="pb-3 border-b border-gray-100">
          <div className="flex items-center gap-2.5">
            <div className="w-9 h-9 rounded-xl bg-blue-50 flex items-center justify-center text-blue-600 border border-blue-100 shadow-xs">
              <Users className="w-5 h-5" />
            </div>
            <div>
              <DialogTitle className="text-lg font-semibold text-gray-900 flex items-center gap-2">
                Name Your Speakers
                <span className="text-xs font-normal px-2 py-0.5 rounded-full bg-blue-50 text-blue-700 border border-blue-200">
                  {uniqueSpeakers.length} Detected
                </span>
              </DialogTitle>
              <DialogDescription className="text-xs text-gray-500 mt-0.5">
                Listen to voice clips to identify who was speaking and replace generic speaker labels.
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        {/* Speakers List */}
        <div className="flex-1 overflow-y-auto py-4 space-y-3.5 pr-1">
          {uniqueSpeakers.map((speaker, index) => {
            const isLoading = !!loadingAudio[speaker];
            const isPlaying = playingSpeaker === speaker;
            const currentValue = renames[speaker] ?? '';

            return (
              <div
                key={speaker}
                className="p-3.5 rounded-xl border border-gray-200/80 bg-white hover:border-gray-300 transition-all shadow-xs space-y-2.5"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span
                      className={`inline-flex items-center px-2.5 py-1 rounded-md text-xs font-semibold ${getSpeakerColorClass(
                        speaker
                      )}`}
                    >
                      {speaker}
                    </span>
                    <span className="text-xs text-gray-400">Speaker #{index + 1}</span>
                  </div>

                  {/* Audio Sample Button */}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => handleTogglePlaySample(speaker)}
                    disabled={isLoading}
                    className={`h-8 px-3 text-xs gap-1.5 transition-all ${
                      isPlaying
                        ? 'bg-amber-50 text-amber-700 border-amber-300 hover:bg-amber-100'
                        : 'text-gray-700 hover:bg-gray-100 border-gray-200'
                    }`}
                  >
                    {isLoading ? (
                      <>
                        <Loader2 className="w-3.5 h-3.5 animate-spin text-blue-500" />
                        <span>Loading...</span>
                      </>
                    ) : isPlaying ? (
                      <>
                        <Square className="w-3.5 h-3.5 fill-current" />
                        <span>Stop Voice</span>
                      </>
                    ) : (
                      <>
                        <Play className="w-3.5 h-3.5 fill-current" />
                        <span>Play Sample</span>
                      </>
                    )}
                  </Button>
                </div>

                {/* Name Input */}
                <div className="space-y-1">
                  <Label htmlFor={`speaker-input-${index}`} className="text-[11px] font-medium text-gray-500">
                    Real Name or Display Name
                  </Label>
                  <Input
                    id={`speaker-input-${index}`}
                    value={currentValue}
                    onChange={(e) => handleNameChange(speaker, e.target.value)}
                    placeholder={`e.g. John Doe, Sarah...`}
                    className="h-9 text-sm rounded-lg border-gray-200 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        handleSave();
                      }
                    }}
                  />
                </div>
              </div>
            );
          })}
        </div>

        {/* Footer */}
        <DialogFooter className="pt-3 border-t border-gray-100 flex items-center justify-between sm:justify-between w-full">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onOpenChange(false)}
            disabled={isSaving}
            className="text-gray-500 hover:text-gray-700 text-xs"
          >
            Skip for now
          </Button>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => onOpenChange(false)}
              disabled={isSaving}
              className="text-xs"
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={handleSave}
              disabled={isSaving}
              className="bg-blue-600 hover:bg-blue-700 text-white text-xs gap-1.5 shadow-xs"
            >
              {isSaving ? (
                <>
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  <span>Saving...</span>
                </>
              ) : (
                <>
                  <Check className="w-3.5 h-3.5" />
                  <span>Save Names</span>
                </>
              )}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

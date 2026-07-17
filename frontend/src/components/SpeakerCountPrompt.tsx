'use client';

import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';
import { Label } from './ui/label';
import { LANGUAGES } from '@/constants/languages';
import { toast } from 'sonner';

interface SpeakerCountPromptProps {
  open: boolean;
  defaultValue: number;
  onConfirm: (count: number) => void;
  onCancel: () => void;
  selectedProvider?: string;
}

export function SpeakerCountPrompt({
  open,
  defaultValue,
  onConfirm,
  onCancel,
  selectedProvider,
}: SpeakerCountPromptProps) {
  const [value, setValue] = useState(String(defaultValue));
  const [language, setLanguage] = useState('pt');

  // Reset value when dialog opens
  useEffect(() => {
    if (open) {
      setValue(String(defaultValue));
      // Load persisted language from backend config
      loadLanguageFromConfig();
    }
  }, [open, defaultValue]);

  const loadLanguageFromConfig = useCallback(async () => {
    try {
      const config = await invoke<any>('api_get_transcript_config');
      if (config && config.meeting_language) {
        setLanguage(config.meeting_language);
      } else {
        setLanguage('pt');
      }
    } catch (err) {
      console.error('Failed to load language config:', err);
      setLanguage('pt');
    }
  }, []);

  const handleConfirm = async () => {
    let count = parseInt(value, 10);

    // Validation: empty, NaN, <1 → 2; >20 → 20
    if (isNaN(count) || count < 1) {
      count = 2;
    } else if (count > 20) {
      count = 20;
    }

    // Persist language to config before confirming
    try {
      const currentConfig = await invoke<any>('api_get_transcript_config');
      await invoke('api_save_transcript_config', {
        provider: currentConfig?.provider || 'localWhisper',
        model: currentConfig?.model || 'large-v3',
        apiKey: currentConfig?.apiKey || null,
        meeting_language: language,
        whisper_initial_prompt: currentConfig?.whisper_initial_prompt || null,
      });
    } catch (err) {
      console.error('Failed to save language config:', err);
      toast.error('Failed to save language setting');
      return;
    }

    onConfirm(count);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleConfirm();
    } else if (e.key === 'Escape') {
      onCancel();
    }
  };

  return (
    <Dialog open={open} onOpenChange={(isOpen) => {
      if (!isOpen) onCancel();
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Recording Settings</DialogTitle>
          <DialogDescription>
            Configure speaker count and meeting language for accurate transcription.
          </DialogDescription>
        </DialogHeader>

        <div className="py-4 space-y-4">
          <div>
            <Label className="text-sm font-medium text-gray-700 mb-1 block">
              Expected speakers
            </Label>
            <Input
              type="number"
              min="1"
              max="20"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="2"
              className="w-full"
            />
            <p className="text-xs text-gray-500 mt-1">
              Valid range: 1–20 speakers. Empty or invalid input defaults to 2.
            </p>
          </div>

          <div>
            <Label className="text-sm font-medium text-gray-700 mb-1 block">
              Meeting language
            </Label>
            <Select value={language} onValueChange={setLanguage}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select language" />
              </SelectTrigger>
              <SelectContent className="max-h-60">
                {LANGUAGES.map((lang) => (
                  <SelectItem key={lang.code} value={lang.code}>
                    {lang.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-gray-500 mt-1">
              Set to a specific language for better accuracy, or use auto-detect
            </p>
            {selectedProvider === 'parakeet' && language !== 'auto' && (
              <div className="mt-2 p-2 bg-amber-50 border border-amber-200 rounded text-xs text-amber-800">
                ⚠️ Parakeet doesn't respect a fixed language — use Whisper for language-forced transcription
              </div>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={handleConfirm}>
            Start Recording
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

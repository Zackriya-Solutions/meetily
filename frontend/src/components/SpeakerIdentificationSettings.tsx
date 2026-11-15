"use client";

import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { Progress } from '@/components/ui/progress';
import {
  SpeakerIdentificationStatus,
  deleteSpeakerModel,
  downloadSpeakerModel,
  getSpeakerIdentificationStatus,
  setSpeakerIdentificationEnabled,
} from '@/lib/speakers';

/**
 * Speaker identification settings: on/off switch and the optional local
 * voiceprint model that tells individual speakers apart.
 */
export function SpeakerIdentificationSettings() {
  const [status, setStatus] = useState<SpeakerIdentificationStatus | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await getSpeakerIdentificationStatus();
      setStatus(next);
      if (!next.downloading) setProgress(null);
    } catch (error) {
      console.error('Failed to load speaker identification status:', error);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = listen<{ percent: number }>('speaker-model-download-progress', (event) => {
      setProgress(event.payload.percent);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const handleToggle = async (enabled: boolean) => {
    setSaving(true);
    try {
      await setSpeakerIdentificationEnabled(enabled);
      await refresh();
    } catch (error) {
      toast.error(typeof error === 'string' ? error : 'Could not save the setting.');
    } finally {
      setSaving(false);
    }
  };

  const handleDownload = async () => {
    setProgress(0);
    setStatus((s) => (s ? { ...s, downloading: true } : s));
    try {
      await downloadSpeakerModel();
      toast.success('Voice model installed. Speakers will be identified in your next recording.');
    } catch (error) {
      toast.error(typeof error === 'string' ? error : 'Voice model download failed.');
    } finally {
      await refresh();
    }
  };

  const handleDelete = async () => {
    try {
      await deleteSpeakerModel();
      await refresh();
    } catch (error) {
      toast.error(typeof error === 'string' ? error : 'Could not delete the voice model.');
    }
  };

  if (!status) return null;

  const sizeMb = Math.round(status.model_size_bytes / (1024 * 1024));

  return (
    <div className="space-y-4 mt-8">
      <div>
        <h3 className="text-lg font-semibold mb-2">Speaker Identification</h3>
        <p className="text-sm text-gray-600">
          Label who said what in transcripts. Your own microphone is labelled &quot;Me&quot;; other voices are
          grouped by voiceprint into Speaker 1, Speaker 2, and so on. You can rename speakers on the meeting page.
          Everything runs on this device.
        </p>
      </div>

      <div className="flex items-center justify-between p-4 border rounded-lg">
        <div className="flex-1">
          <div className="font-medium">Identify speakers</div>
          <div className="text-sm text-gray-600">
            Applies to new recordings, imports, and re-transcriptions
          </div>
        </div>
        <Switch checked={status.enabled} onCheckedChange={handleToggle} disabled={saving} />
      </div>

      {status.enabled && (
        <div className="p-4 border rounded-lg space-y-3">
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1">
              <div className="font-medium">Voice model (CAM++, {sizeMb} MB)</div>
              <div className="text-sm text-gray-600">
                {status.model_downloaded
                  ? 'Installed. Different voices are told apart, including people in the same room.'
                  : 'Not installed. Without it, transcripts only separate “Me” (microphone) from “Others” (system audio).'}
              </div>
            </div>
            {status.model_downloaded ? (
              <button
                onClick={handleDelete}
                className="px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
              >
                Remove
              </button>
            ) : (
              <button
                onClick={handleDownload}
                disabled={status.downloading}
                className="px-3 py-2 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60 transition-colors"
              >
                {status.downloading ? 'Downloading…' : 'Download'}
              </button>
            )}
          </div>
          {status.downloading && progress !== null && (
            <div className="space-y-1">
              <Progress value={progress} />
              <div className="text-xs text-gray-500">{progress}%</div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

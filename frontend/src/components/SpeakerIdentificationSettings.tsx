'use client';

// Settings section for speaker identification (diarization).
// Toggle persists via the diarization_set_enabled Tauri command;
// the embedding model (~28 MB) downloads on demand with progress.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button } from './ui/button';
import { Label } from './ui/label';
import { Switch } from './ui/switch';
import { Progress } from './ui/progress';
import { Input } from './ui/input';
import { Download, Mic2, Pencil, Trash2 } from 'lucide-react';
import { toast } from 'sonner';

interface DiarizationStatus {
  enabled: boolean;
  model_present: boolean;
  model_filename: string;
  expected_speakers?: number;
  segmentation_model_present?: boolean;
  embedding_model_present?: boolean;
}

interface VoiceProfile {
  id: string;
  name: string;
}

interface DownloadProgressEvent {
  downloaded_bytes: number;
  total_bytes: number;
  percent: number;
}

export function SpeakerIdentificationSettings() {
  const [status, setStatus] = useState<DiarizationStatus | null>(null);
  const [isDownloading, setIsDownloading] = useState(false);
  const [downloadPercent, setDownloadPercent] = useState(0);
  const [profiles, setProfiles] = useState<VoiceProfile[]>([]);

  const refreshStatus = useCallback(async () => {
    try {
      const result = await invoke<DiarizationStatus>('diarization_get_status');
      setStatus(result);
    } catch (err) {
      console.error('Failed to fetch diarization status:', err);
    }
  }, []);

  const refreshProfiles = useCallback(async () => {
    try {
      const result = await invoke<VoiceProfile[]>('diarization_list_profiles');
      setProfiles(result);
    } catch (err) {
      console.error('Failed to fetch voice profiles:', err);
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    refreshProfiles();
  }, [refreshStatus, refreshProfiles]);

  // Inline rename state (window.prompt is not supported in Tauri's webview)
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState('');

  const handleRenameProfile = async (profile: VoiceProfile) => {
    const name = editName.trim();
    setEditingId(null);
    if (!name || name === profile.name) return;
    try {
      await invoke('diarization_rename_profile', { id: profile.id, name });
      await refreshProfiles();
      toast.success('Voice profile renamed');
    } catch (err) {
      toast.error(`Failed to rename profile: ${err}`);
    }
  };

  const handleDeleteProfile = async (profile: VoiceProfile) => {
    try {
      await invoke('diarization_delete_profile', { id: profile.id });
      await refreshProfiles();
      toast.success(`Forgot voice "${profile.name}"`);
    } catch (err) {
      toast.error(`Failed to delete profile: ${err}`);
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      unlisten = await listen<DownloadProgressEvent>(
        'diarization-model-download-progress',
        (event) => {
          setDownloadPercent(event.payload.percent);
        }
      );
    };
    setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const handleToggle = async (enabled: boolean) => {
    try {
      await invoke('diarization_set_enabled', { enabled });
      setStatus((prev) => (prev ? { ...prev, enabled } : prev));
      if (enabled && status && !status.model_present) {
        toast.info('Download the speaker model to activate speaker labels.');
      } else if (enabled) {
        toast.success('Speaker identification will be active on your next recording.');
      }
    } catch (err) {
      console.error('Failed to update speaker identification setting:', err);
      toast.error('Failed to update speaker identification setting');
    }
  };

  const handleDownload = async () => {
    setIsDownloading(true);
    setDownloadPercent(0);
    try {
      await invoke('diarization_download_model');
      toast.success('Speaker model downloaded');
      await refreshStatus();
    } catch (err) {
      console.error('Speaker model download failed:', err);
      toast.error(`Speaker model download failed: ${err}`);
    } finally {
      setIsDownloading(false);
    }
  };

  const handleExpectedSpeakersChange = async (value: string) => {
    try {
      let count = parseInt(value, 10);
      if (isNaN(count) || count < 1) {
        count = 2;
      } else if (count > 20) {
        count = 20;
      }
      await invoke('diarization_set_expected_speakers', { count });
      setStatus((prev) => (prev ? { ...prev, expected_speakers: count } : prev));
      toast.success(`Default speaker count set to ${count}`);
    } catch (err) {
      console.error('Failed to set expected speakers:', err);
      toast.error('Failed to update default speaker count');
    }
  };

  if (!status) return null;

  return (
    <div className="mt-6 border-t pt-4">
      <div className="flex items-center justify-between">
        <div>
          <Label className="flex items-center gap-2 text-sm font-medium text-gray-700">
            <Mic2 className="h-4 w-4" />
            Speaker identification
            <span className="text-xs font-normal text-amber-600 bg-amber-50 px-1.5 py-0.5 rounded">
              Experimental
            </span>
          </Label>
          <p className="text-xs text-gray-500 mt-1">
            Label who said what in transcripts. Runs fully on-device; voice data never
            leaves your computer.
          </p>
        </div>
        <Switch checked={status.enabled} onCheckedChange={handleToggle} />
      </div>

      {status.enabled && (!status.model_present || !status.segmentation_model_present || !status.embedding_model_present) && (
        <div className="mt-3">
          {isDownloading ? (
            <div className="space-y-1">
              <Progress value={downloadPercent} />
              <p className="text-xs text-gray-500">Downloading models… {downloadPercent}%</p>
            </div>
          ) : (
            <div className="space-y-2">
              {!status.segmentation_model_present && (
                <div className="flex items-center justify-between p-2 bg-gray-50 rounded border border-gray-200">
                  <span className="text-xs text-gray-700">Voice segmentation model</span>
                  <Button variant="ghost" size="sm" onClick={handleDownload} className="h-8">
                    <Download className="h-4 w-4 mr-1" />
                    Download
                  </Button>
                </div>
              )}
              {!status.embedding_model_present && (
                <div className="flex items-center justify-between p-2 bg-gray-50 rounded border border-gray-200">
                  <span className="text-xs text-gray-700">Speaker embedding model</span>
                  <Button variant="ghost" size="sm" onClick={handleDownload} className="h-8">
                    <Download className="h-4 w-4 mr-1" />
                    Download
                  </Button>
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {status.enabled && (status.model_present || (status.segmentation_model_present && status.embedding_model_present)) && (
        <div className="mt-3">
          <div className="space-y-2 mb-4">
            {status.segmentation_model_present && (
              <div className="flex items-center gap-2">
                <div className="h-2 w-2 rounded-full bg-green-500" />
                <span className="text-xs text-gray-600">Voice segmentation model installed</span>
              </div>
            )}
            {status.embedding_model_present && (
              <div className="flex items-center gap-2">
                <div className="h-2 w-2 rounded-full bg-green-500" />
                <span className="text-xs text-gray-600">Speaker embedding model installed</span>
              </div>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Label className="text-xs font-medium text-gray-500">
              Default speakers per recording:
            </Label>
            <Input
              type="number"
              min="1"
              max="20"
              value={status.expected_speakers ?? 2}
              onChange={(e) => handleExpectedSpeakersChange(e.target.value)}
              className="w-16 h-8 text-sm px-2"
            />
          </div>
        </div>
      )}

      {status.enabled && profiles.length > 0 && (
        <div className="mt-4">
          <Label className="text-xs font-medium text-gray-500 uppercase tracking-wide">
            Remembered voices
          </Label>
          <ul className="mt-2 space-y-1">
            {profiles.map((profile) => (
              <li
                key={profile.id}
                className="flex items-center justify-between text-sm text-gray-700 bg-gray-50 rounded px-2 py-1"
              >
                {editingId === profile.id ? (
                  <input
                    autoFocus
                    className="flex-1 mr-2 px-1 py-0.5 text-sm border border-gray-300 rounded"
                    value={editName}
                    onChange={(e) => setEditName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleRenameProfile(profile);
                      if (e.key === 'Escape') setEditingId(null);
                    }}
                    onBlur={() => handleRenameProfile(profile)}
                  />
                ) : (
                  <span>{profile.name}</span>
                )}
                <span className="flex gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6"
                    title="Rename"
                    onClick={() => {
                      setEditingId(profile.id);
                      setEditName(profile.name);
                    }}
                  >
                    <Pencil className="h-3 w-3" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6"
                    title="Forget this voice"
                    onClick={() => handleDeleteProfile(profile)}
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

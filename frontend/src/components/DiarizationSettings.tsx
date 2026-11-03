'use client';

import { useState, useEffect, useRef } from 'react';
import { diarizationService } from '@/services/diarizationService';
import {
  DiarizationConfig,
  DiarizationModelInfo,
  DiarizationDownloadProgress,
  DiarizationStreamingPreset,
} from '@/types';
import { Switch } from './ui/switch';
import { Button } from './ui/button';
import { Label } from './ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';
import { Progress } from './ui/progress';
import {
  Users,
  Download,
  CheckCircle2,
  AlertCircle,
  X,
  Trash2,
  FolderOpen,
  Cpu,
  RefreshCw,
} from 'lucide-react';
import { toast } from 'sonner';

export function DiarizationSettings() {
  const [config, setConfig] = useState<DiarizationConfig>({
    enabled: false,
    preset: 'Offline',
    max_speakers: 4,
    threshold: 0.5,
  });
  const [modelInfo, setModelInfo] = useState<DiarizationModelInfo | null>(null);
  const [isLoaded, setIsLoaded] = useState<boolean>(false);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [downloadProgress, setDownloadProgress] = useState<DiarizationDownloadProgress | null>(null);
  const [isDownloading, setIsDownloading] = useState<boolean>(false);
  const [isActionInProgress, setIsActionInProgress] = useState<boolean>(false);
  const pollingRef = useRef<NodeJS.Timeout | null>(null);

  const fetchState = async () => {
    try {
      await diarizationService.init();
      const [settings, info, loaded] = await Promise.all([
        diarizationService.getSettings(),
        diarizationService.getModelInfo(),
        diarizationService.isModelLoaded(),
      ]);
      setConfig(settings);
      setModelInfo(info);
      setIsLoaded(loaded);
    } catch (err) {
      console.error('Failed to fetch diarization state:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchState();

    // Subscribe to download events
    let unlistenProgress: (() => void) | null = null;
    let unlistenComplete: (() => void) | null = null;

    diarizationService
      .onDownloadProgress((progress) => {
        setIsDownloading(true);
        setDownloadProgress(progress);
      })
      .then((unlisten) => {
        unlistenProgress = unlisten;
      });

    diarizationService
      .onDownloadComplete(() => {
        setIsDownloading(false);
        setDownloadProgress(null);
        toast.success('Nemotron-3 Diarization model downloaded successfully!');
        fetchState();
      })
      .then((unlisten) => {
        unlistenComplete = unlisten;
      });

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenComplete) unlistenComplete();
      if (pollingRef.current) clearInterval(pollingRef.current);
    };
  }, []);

  const handleToggleEnabled = async (checked: boolean) => {
    const updated = { ...config, enabled: checked };
    setConfig(updated);
    try {
      await diarizationService.saveSettings(
        checked,
        config.preset,
        config.max_speakers,
        config.threshold
      );
      if (checked) {
        toast.success('Speaker Diarization enabled');
        if (modelInfo && modelInfo.status === 'Available' && !isLoaded) {
          diarizationService.loadModel().then(() => setIsLoaded(true));
        }
      } else {
        toast.info('Speaker Diarization disabled');
      }
    } catch (err) {
      console.error('Failed to save diarization settings:', err);
      toast.error('Failed to update diarization settings');
      setConfig(config);
    }
  };

  const handlePresetChange = async (val: string) => {
    const preset = val as DiarizationStreamingPreset;
    const updated = { ...config, preset };
    setConfig(updated);
    try {
      await diarizationService.saveSettings(
        config.enabled,
        preset,
        config.max_speakers,
        config.threshold
      );
      toast.success(`Diarization preset updated to ${preset}`);
    } catch (err) {
      console.error('Failed to update preset:', err);
    }
  };

  const handleMaxSpeakersChange = async (val: string) => {
    const maxSpeakers = parseInt(val, 10);
    const updated = { ...config, max_speakers: maxSpeakers };
    setConfig(updated);
    try {
      await diarizationService.saveSettings(
        config.enabled,
        config.preset,
        maxSpeakers,
        config.threshold
      );
      toast.success(`Max speakers set to ${maxSpeakers}`);
    } catch (err) {
      console.error('Failed to update max speakers:', err);
      toast.error('Failed to update max speakers');
    }
  };

  const handleDownload = async () => {
    setIsDownloading(true);
    try {
      toast.info('Starting Nemotron-3 Diarization model download (~382 MB)...');
      await diarizationService.downloadModel();
    } catch (err: any) {
      console.error('Download failed:', err);
      toast.error(`Download failed: ${err}`);
      setIsDownloading(false);
      setDownloadProgress(null);
    }
  };

  const handleCancelDownload = async () => {
    try {
      await diarizationService.cancelDownload();
      setIsDownloading(false);
      setDownloadProgress(null);
      toast.info('Download cancelled');
      fetchState();
    } catch (err) {
      console.error('Cancel failed:', err);
    }
  };

  const handleDeleteModel = async () => {
    if (!confirm('Are you sure you want to delete the downloaded Nemotron-3 model?')) {
      return;
    }
    setIsActionInProgress(true);
    try {
      await diarizationService.deleteModel();
      toast.success('Model deleted from disk');
      await fetchState();
    } catch (err) {
      console.error('Delete failed:', err);
      toast.error('Failed to delete model');
    } finally {
      setIsActionInProgress(false);
    }
  };

  const handleOpenFolder = async () => {
    try {
      await diarizationService.openModelsFolder();
    } catch (err) {
      console.error('Failed to open models folder:', err);
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center space-x-2 py-4 text-sm text-gray-500">
        <RefreshCw className="h-4 w-4 animate-spin text-blue-500" />
        <span>Loading diarization settings...</span>
      </div>
    );
  }

  const isModelAvailable = modelInfo?.status === 'Available';

  return (
    <div className="border border-gray-200 rounded-xl p-4 bg-white shadow-xs space-y-4">
      {/* Top Banner: Toggle & Title */}
      <div className="flex items-start justify-between">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <div className="p-1.5 bg-blue-50 text-blue-600 rounded-lg">
              <Users className="h-4 w-4" />
            </div>
            <h4 className="text-sm font-semibold text-gray-900">
              Speaker Diarization (Nemotron-3)
            </h4>
            <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium bg-emerald-50 text-emerald-700 border border-emerald-200">
              <Cpu className="h-3 w-3" />
              100% On-Device
            </span>
          </div>
          <p className="text-xs text-gray-500">
            Identify and separate "who spoke when" across up to 8 distinct speakers using NVIDIA's
            Nemotron-3 architecture.
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={handleToggleEnabled}
          aria-label="Toggle speaker diarization"
        />
      </div>

      {config.enabled && (
        <div className="pt-2 border-t border-gray-100 space-y-4 animate-in fade-in duration-200">
          {/* Model Status Card */}
          <div className="bg-gray-50 border border-gray-200/80 rounded-lg p-3 space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-gray-800">
                  Nemotron-3 Diarization (Sortformer v3)
                </span>
                <span className="text-xs text-gray-400">~382 MB</span>
              </div>
              <div>
                {isDownloading ? (
                  <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium bg-blue-50 text-blue-700">
                    <RefreshCw className="h-3 w-3 animate-spin" />
                    Downloading...
                  </span>
                ) : isModelAvailable ? (
                  <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium bg-emerald-50 text-emerald-700">
                    <CheckCircle2 className="h-3 w-3" />
                    {isLoaded ? 'Loaded & Active' : 'Downloaded (Ready)'}
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium bg-amber-50 text-amber-700">
                    <AlertCircle className="h-3 w-3" />
                    Model Required
                  </span>
                )}
              </div>
            </div>

            {/* Download Progress */}
            {isDownloading && downloadProgress && (
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs text-gray-500">
                  <span>
                    {downloadProgress.downloaded_mb.toFixed(1)} MB /{' '}
                    {downloadProgress.total_mb.toFixed(1)} MB
                  </span>
                  <span>
                    {downloadProgress.percent}% ({downloadProgress.speed_mbps.toFixed(1)} MB/s)
                  </span>
                </div>
                <Progress value={downloadProgress.percent} className="h-2" />
                <div className="flex justify-end pt-1">
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={handleCancelDownload}
                    className="h-7 text-xs text-red-600 hover:text-red-700"
                  >
                    <X className="h-3 w-3 mr-1" />
                    Cancel
                  </Button>
                </div>
              </div>
            )}

            {/* Actions if model is missing */}
            {!isModelAvailable && !isDownloading && (
              <div className="flex items-center justify-between pt-1">
                <p className="text-xs text-gray-500">
                  Download the local ONNX model file to enable on-device speaker attribution.
                </p>
                <Button
                  size="sm"
                  onClick={handleDownload}
                  className="bg-blue-600 hover:bg-blue-700 text-white text-xs h-8 px-3"
                >
                  <Download className="h-3.5 w-3.5 mr-1.5" />
                  Download Model
                </Button>
              </div>
            )}

            {/* Actions if model is available */}
            {isModelAvailable && !isDownloading && (
              <div className="flex items-center justify-between pt-1 text-xs text-gray-500">
                <div className="flex items-center gap-3">
                  <button
                    onClick={handleOpenFolder}
                    className="flex items-center gap-1 hover:text-blue-600 transition-colors"
                  >
                    <FolderOpen className="h-3.5 w-3.5" />
                    Open folder
                  </button>
                  <button
                    onClick={handleDeleteModel}
                    disabled={isActionInProgress}
                    className="flex items-center gap-1 text-red-500 hover:text-red-600 transition-colors"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                    Delete model
                  </button>
                </div>
              </div>
            )}
          </div>

          {/* Diarization Settings Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
            {/* Preset Selector */}
            <div className="space-y-1">
              <Label className="text-xs font-medium text-gray-700">Latency Mode</Label>
              <Select value={config.preset} onValueChange={handlePresetChange}>
                <SelectTrigger className="h-8 text-xs focus:ring-1 focus:ring-blue-500">
                  <SelectValue placeholder="Select mode" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Offline">
                    🎯 Offline / High Accuracy (Recommended)
                  </SelectItem>
                  <SelectItem value="LowLatency">⚡ Low Latency (~1.0s buffer)</SelectItem>
                  <SelectItem value="VeryLowLatency">🚀 Very Low Latency (~0.6s buffer)</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {/* Max Speakers Selector */}
            <div className="space-y-1">
              <Label className="text-xs font-medium text-gray-700">Max Speakers</Label>
              <Select
                value={config.max_speakers.toString()}
                onValueChange={handleMaxSpeakersChange}
              >
                <SelectTrigger className="h-8 text-xs focus:ring-1 focus:ring-blue-500">
                  <SelectValue placeholder="Speakers" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="2">2 Speakers (1-on-1 meeting)</SelectItem>
                  <SelectItem value="4">4 Speakers (Standard team)</SelectItem>
                  <SelectItem value="6">6 Speakers (Medium group)</SelectItem>
                  <SelectItem value="8">8 Speakers (Large meeting)</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

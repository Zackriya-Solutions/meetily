'use client';

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Download, CheckCircle2, Loader2, AlertCircle, Users } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';

interface DiarizationModelStatus {
  segmentation_available: boolean;
  embedding_available: boolean;
  both_available: boolean;
}

interface DownloadState {
  status: 'idle' | 'downloading' | 'completed' | 'error';
  progress: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
  error?: string;
}

const MODEL_INFO = {
  segmentation: {
    label: 'Speaker Segmentation',
    description: 'Detects when speech changes between speakers (~17 MB)',
    totalMb: 17,
  },
  embedding: {
    label: 'Speaker Embedding',
    description: 'Identifies unique speaker voices (~200 MB)',
    totalMb: 200,
  },
};

export function DiarizationSettings() {
  const [modelStatus, setModelStatus] = useState<DiarizationModelStatus>({
    segmentation_available: false,
    embedding_available: false,
    both_available: false,
  });

  const [segState, setSegState] = useState<DownloadState>({ status: 'idle', progress: 0, downloadedMb: 0, totalMb: MODEL_INFO.segmentation.totalMb, speedMbps: 0 });
  const [embState, setEmbState] = useState<DownloadState>({ status: 'idle', progress: 0, downloadedMb: 0, totalMb: MODEL_INFO.embedding.totalMb, speedMbps: 0 });

  useEffect(() => {
    loadStatus();
  }, []);

  const loadStatus = async () => {
    try {
      const status = await invoke<DiarizationModelStatus>('diarization_check_models');
      setModelStatus(status);
      if (status.segmentation_available) {
        setSegState(prev => ({ ...prev, status: 'completed', progress: 100 }));
      }
      if (status.embedding_available) {
        setEmbState(prev => ({ ...prev, status: 'completed', progress: 100 }));
      }
    } catch (err) {
      console.error('Failed to check diarization models:', err);
    }
  };

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    const setup = async () => {
      const unlistenProgress = await listen<any>('diarization-download-progress', (event) => {
        const { modelType, progress, downloaded_mb, total_mb, speed_mbps } = event.payload;
        const setState = modelType === 'segmentation' ? setSegState : setEmbState;
        setState({
          status: 'downloading',
          progress,
          downloadedMb: downloaded_mb,
          totalMb: total_mb,
          speedMbps: speed_mbps,
        });
      });
      unlisteners.push(unlistenProgress);

      const unlistenComplete = await listen<any>('diarization-download-complete', (event) => {
        const { modelType } = event.payload;
        const setState = modelType === 'segmentation' ? setSegState : setEmbState;
        setState(prev => ({ ...prev, status: 'completed', progress: 100 }));
        setModelStatus(prev => ({
          ...prev,
          [`${modelType}_available`]: true,
          both_available: modelType === 'segmentation' ? prev.embedding_available : prev.segmentation_available,
        }));
        toast.success(`Speaker ${modelType === 'segmentation' ? 'segmentation' : 'embedding'} model downloaded`);
        Analytics.track('model_download_completed', {
          model_name: modelType,
          model_type: 'diarization',
          success: 'true',
        });
        loadStatus();
      });
      unlisteners.push(unlistenComplete);
    };

    setup();
    return () => unlisteners.forEach(fn => fn());
  }, []);

  const handleDownload = async (modelType: 'segmentation' | 'embedding') => {
    const setState = modelType === 'segmentation' ? setSegState : setEmbState;
    setState(prev => ({ ...prev, status: 'downloading', progress: 0, error: undefined }));
    try {
      await Analytics.track('model_download_started', {
        model_name: modelType,
        model_type: 'diarization',
      });
      await invoke('diarization_download_model', { modelType });
    } catch (err: any) {
      setState(prev => ({ ...prev, status: 'error', error: String(err) }));
      toast.error(`Failed to download ${modelType} model`, { description: String(err) });
      Analytics.track('model_download_completed', {
        model_name: modelType,
        model_type: 'diarization',
        success: 'false',
        error: String(err),
      });
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 mb-1">
        <Users className="w-4 h-4 text-indigo-500" />
        <h3 className="text-sm font-semibold text-gray-800">Speaker Identification</h3>
        {modelStatus.both_available && (
          <span className="text-xs bg-green-100 text-green-700 px-2 py-0.5 rounded-full">Ready</span>
        )}
      </div>
      <p className="text-xs text-gray-500">
        Download these models to automatically identify different speakers when re-transcribing meetings.
        Labels like "Speaker 1", "Speaker 2" will be added to each transcript line.
      </p>

      {(['segmentation', 'embedding'] as const).map(modelType => {
        const info = MODEL_INFO[modelType];
        const state = modelType === 'segmentation' ? segState : embState;
        const isAvailable = modelType === 'segmentation'
          ? modelStatus.segmentation_available
          : modelStatus.embedding_available;

        return (
          <div key={modelType} className="border border-gray-200 rounded-lg p-3 space-y-2">
            <div className="flex items-start justify-between gap-2">
              <div className="flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-gray-700">{info.label}</span>
                  {isAvailable && <CheckCircle2 className="w-4 h-4 text-green-500" />}
                </div>
                <p className="text-xs text-gray-400">{info.description}</p>
              </div>
              {!isAvailable && state.status !== 'downloading' && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => handleDownload(modelType)}
                  className="flex-shrink-0"
                >
                  <Download className="w-3 h-3 mr-1" />
                  Download
                </Button>
              )}
            </div>

            {state.status === 'downloading' && (
              <div className="space-y-1">
                <Progress value={state.progress} className="h-1.5" />
                <div className="flex justify-between text-xs text-gray-400">
                  <span>{state.downloadedMb.toFixed(1)} / {state.totalMb.toFixed(1)} MB</span>
                  <span>{state.speedMbps.toFixed(1)} MB/s</span>
                </div>
              </div>
            )}

            {state.status === 'error' && (
              <div className="flex items-center gap-1 text-xs text-red-500">
                <AlertCircle className="w-3 h-3" />
                {state.error || 'Download failed'}
                <Button size="sm" variant="ghost" className="h-5 text-xs ml-1" onClick={() => handleDownload(modelType)}>
                  Retry
                </Button>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}


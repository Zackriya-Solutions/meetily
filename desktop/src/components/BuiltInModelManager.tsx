'use client';

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { cn } from '@/lib/utils';
import { Download, RefreshCw, BadgeAlert, Trash2, FileUp } from 'lucide-react';
import { toast } from 'sonner';

interface ModelInfo {
  name: string;
  display_name: string;
  status: {
    type: 'not_downloaded' | 'downloading' | 'available' | 'corrupted' | 'error';
    progress?: number;
  };
  size_mb: number;
  context_size: number;
  compatibility: 'recommended' | 'compatible' | 'may_be_slow' | 'not_recommended';
  memory_estimate_gb: number;
  description: string;
  gguf_file: string;
}

interface DownloadProgressInfo {
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
}

interface BuiltInModelManagerProps {
  selectedModel: string;
  onModelSelect: (model: string) => void;
}

export function BuiltInModelManager({ selectedModel, onModelSelect }: BuiltInModelManagerProps) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [hasFetched, setHasFetched] = useState<boolean>(false);
  const [downloadProgress, setDownloadProgress] = useState<Record<string, number>>({});
  const [downloadProgressInfo, setDownloadProgressInfo] = useState<Record<string, DownloadProgressInfo>>({});
  const [downloadingModels, setDownloadingModels] = useState<Set<string>>(new Set());
  const [importingModels, setImportingModels] = useState<Set<string>>(new Set());

  const getCompatibilityMeta = (compatibility: ModelInfo['compatibility']) => {
    switch (compatibility) {
      case 'recommended':
        return { label: 'Recommended', className: 'bg-green-100 text-green-700' };
      case 'compatible':
        return { label: 'Compatible', className: 'bg-blue-100 text-blue-700' };
      case 'may_be_slow':
        return { label: 'May Be Slow', className: 'bg-amber-100 text-amber-700' };
      case 'not_recommended':
        return { label: 'Not Recommended', className: 'bg-red-100 text-red-700' };
      default:
        return { label: 'Compatible', className: 'bg-gray-100 text-gray-700' };
    }
  };

  const fetchModels = async () => {
    try {
      setIsLoading(true);
      const data = (await invoke('builtin_ai_list_models')) as ModelInfo[];
      setModels(data);

      // Auto-select first available model if none selected
      if (data.length > 0 && !selectedModel) {
        const firstAvailable = data.find((m) => m.status.type === 'available');
        if (firstAvailable) {
          onModelSelect(firstAvailable.name);
        }
      }
    } catch (error) {
      console.error('Failed to fetch MeetFree Built-in models:', error);
      toast.error('Failed to load models');
    } finally {
      setIsLoading(false);
      setHasFetched(true);
    }
  };

  useEffect(() => {
    fetchModels();
  }, []);

  // Listen for download progress events
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      unlisten = await listen('builtin-ai-download-progress', (event: any) => {
        const { model, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;

        // Update percentage progress
        setDownloadProgress((prev) => ({
          ...prev,
          [model]: progress,
        }));

        // Update detailed progress info (MB, speed)
        setDownloadProgressInfo((prev) => ({
          ...prev,
          [model]: {
            downloadedMb: downloaded_mb ?? 0,
            totalMb: total_mb ?? 0,
            speedMbps: speed_mbps ?? 0,
          },
        }));

        // Handle downloading status - restore downloadingModels state on modal reopen
        if (status === 'downloading') {
          setDownloadingModels((prev) => {
            if (!prev.has(model)) {
              const newSet = new Set(prev);
              newSet.add(model);
              return newSet;
            }
            return prev;
          });
        }

        // Handle completed status
        if (status === 'completed') {
          setDownloadingModels((prev) => {
            const newSet = new Set(prev);
            newSet.delete(model);
            return newSet;
          });
          // Clean up progress state
          setDownloadProgress((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });
          setDownloadProgressInfo((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });
          // Refresh models list
          fetchModels();
          toast.success(`Model ${model} downloaded successfully`);
        }

        // Handle cancelled status
        if (status === 'cancelled') {
          setDownloadingModels((prev) => {
            const newSet = new Set(prev);
            newSet.delete(model);
            return newSet;
          });
          // Clean up progress state
          setDownloadProgress((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });
          setDownloadProgressInfo((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });
          // Refresh models list
          fetchModels();
        }

        // Handle error status
        if (status === 'error') {
          setDownloadingModels((prev) => {
            const newSet = new Set(prev);
            newSet.delete(model);
            return newSet;
          });
          // Clean up progress state
          setDownloadProgress((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });
          setDownloadProgressInfo((prev) => {
            const next = { ...prev };
            delete next[model];
            return next;
          });

          // Update model status to error locally instead of fetching from backend
          // Backend doesn't persist error status, so fetchModels() would return not_downloaded
          setModels((prevModels) =>
            prevModels.map((m) =>
              m.name === model
                ? {
                    ...m,
                    status: {
                      type: 'error',
                      progress: 0,
                    } as any,
                  }
                : m
            )
          );

          // Don't show error toast here - DownloadProgressToast already handles it
          // Don't call fetchModels() - it would overwrite error status with not_downloaded
        }
      });
    };

    setupListener();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const downloadModel = async (modelName: string) => {
    try {
      // Optimistically add to downloadingModels for immediate UI feedback
      setDownloadingModels((prev) => new Set([...prev, modelName]));

      await invoke('builtin_ai_download_model', { modelName });
    } catch (error) {
      console.error('Failed to download model:', error);

      // Check if this is a cancellation error (starts with "CANCELLED:")
      const errorMsg = String(error);
      if (errorMsg.startsWith('CANCELLED:')) {
        // Cancel handler already removed from downloadingModels
        // Don't show error toast for cancellations - cancel function already shows info toast
        return;
      }

      // For real errors, show toast and remove from downloading
      toast.error(`Failed to download ${modelName}`);

      setDownloadingModels((prev) => {
        const newSet = new Set(prev);
        newSet.delete(modelName);
        return newSet;
      });

      // Refresh model list to get updated Error status from backend
      fetchModels();
    }
  };

  const cancelDownload = async (modelName: string) => {
    try {
      await invoke('builtin_ai_cancel_download', { modelName });
      toast.info(`Download of ${modelName} cancelled`);
      setDownloadingModels((prev) => {
        const newSet = new Set(prev);
        newSet.delete(modelName);
        return newSet;
      });
    } catch (error) {
      console.error('Failed to cancel download:', error);
    }
  };

  const deleteModel = async (modelName: string) => {
    try {
      await invoke('builtin_ai_delete_model', { modelName });
      toast.success(`Model ${modelName} deleted`);
      fetchModels();
    } catch (error) {
      console.error('Failed to delete model:', error);
      toast.error(`Failed to delete ${modelName}`);
    }
  };

  const importModel = async (modelName: string) => {
    setImportingModels((prev) => new Set([...prev, modelName]));
    try {
      const validation = await invoke<{
        model_name: string;
        file_path: string;
        valid: boolean;
        file_size_mb: number;
        expected_size_mb: number;
        issues: string[];
      }>('builtin_ai_validate_model_file', { modelName, filePath: '' });

      if (!validation.valid) {
        const message = validation.issues[0] ?? 'Selected file is not compatible with this model.';
        toast.error(`Import validation failed: ${message}`);
        return;
      }

      await invoke('builtin_ai_import_model_file', {
        modelName,
        filePath: validation.file_path,
      });

      toast.success(`${modelName} imported successfully`);
      await fetchModels();
    } catch (error) {
      const message = String(error);
      if (message.includes('No model file selected')) {
        return;
      }
      console.error('Failed to import model file:', error);
      toast.error(`Failed to import ${modelName}: ${message}`);
    } finally {
      setImportingModels((prev) => {
        const next = new Set(prev);
        next.delete(modelName);
        return next;
      });
    }
  };

  // Don't show loading spinner if we have downloads in progress - show the model list instead
  if (isLoading && downloadingModels.size === 0) {
    return (
      <div className="text-center py-8 text-muted-foreground">
        <RefreshCw className="mx-auto h-8 w-8 animate-spin mb-2" />
        Loading models...
      </div>
    );
  }

  // Only show "no models" message after fetch has completed
  if (hasFetched && models.length === 0) {
    return (
      <Alert>
        <AlertDescription>
          No models found. Download a model to get started with MeetFree Built-in.
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <h4 className="text-sm font-bold">MeetFree Built-in Models</h4>
      </div>

      <div className="grid gap-4">
        {models.map((model) => {
          const progress = downloadProgress[model.name];
          const progressInfo = downloadProgressInfo[model.name];
          const modelIsDownloading = downloadingModels.has(model.name);
          const modelIsImporting = importingModels.has(model.name);
          const isAvailable = model.status.type === 'available';
          const isNotDownloaded = model.status.type === 'not_downloaded';
          const isCorrupted = model.status.type === 'corrupted';
          const isError = model.status.type === 'error';
          const compatibilityMeta = getCompatibilityMeta(model.compatibility);

          return (
            <div
              key={model.name}
              className={cn(
                'p-4 rounded-lg border transition-colors',
                modelIsDownloading
                  ? 'bg-white border-gray-200'
                  : 'bg-card',
                selectedModel === model.name
                  ? 'ring-2 ring-gray-800 border-gray-800'
                  : 'border-gray-200 hover:border-gray-300',
                isAvailable && !modelIsDownloading && !modelIsImporting && 'cursor-pointer'
              )}
              onClick={() => {
                if (isAvailable && !modelIsDownloading && !modelIsImporting) {
                  onModelSelect(model.name);
                }
              }}
            >
              <div className="flex items-start justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-base font-bold text-gray-900">{model.display_name || model.name}</span>
                    {isAvailable && (
                      <>
                        <span className="text-xs text-green-600 font-medium flex items-center gap-1">
                          <span className="w-2 h-2 rounded-full bg-green-600"></span>
                          Ready
                        </span>
                        {selectedModel === model.name && (
                          <span className="px-2 py-0.5 text-xs font-medium bg-blue-100 text-blue-700 rounded">
                            Selected
                          </span>
                        )}
                      </>
                    )}
                    {isCorrupted && (
                      <span className="px-2 py-0.5 text-xs font-medium bg-red-100 text-red-700 rounded flex items-center gap-1">
                        <BadgeAlert className="w-3 h-3" />
                        Corrupted
                      </span>
                    )}
                    {isError && (
                      <span className="px-2 py-0.5 text-xs font-medium bg-red-100 text-red-700 rounded">
                        Error
                      </span>
                    )}
                    {isNotDownloaded && !modelIsDownloading && (
                      <span className="text-xs text-gray-600 font-medium">
                        Not Downloaded
                      </span>
                    )}
                    {!modelIsDownloading && (
                      <span className={cn('px-2 py-0.5 text-xs font-medium rounded', compatibilityMeta.className)}>
                        {compatibilityMeta.label}
                      </span>
                    )}
                  </div>
                  <div className="text-sm text-gray-600">
                    {model.description && (
                      <p className="mb-1">{model.description}</p>
                    )}
                    {(isError || isCorrupted) && (
                      <p className="mb-1 text-xs text-red-600">
                        {isError && typeof model.status === 'object' && 'Error' in model.status
                          ? (model.status as any).Error
                          : isCorrupted
                          ? 'File is corrupted. Retry download or delete.'
                          : 'An error occurred'}
                      </p>
                    )}
                    <div className="text-xs text-gray-500">
                      <span className="block">~{model.memory_estimate_gb.toFixed(1)} GB RAM suggested</span>
                      <span>{model.size_mb}MB • {model.context_size} tokens</span>
                    </div>
                  </div>
                </div>

                <div className="ml-4 flex items-center gap-2">
                  {/* Not Downloaded - Show Download button */}
                  {isNotDownloaded && !modelIsDownloading && (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        className="min-w-[100px]"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          downloadModel(model.name);
                        }}
                      >
                        <Download className="mr-2 h-4 w-4" />
                        Download
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          importModel(model.name);
                        }}
                      >
                        <FileUp className="mr-2 h-4 w-4" />
                        {modelIsImporting ? 'Importing...' : 'Import File'}
                      </Button>
                    </>
                  )}

                  {/* Downloading - Show Cancel button */}
                  {modelIsDownloading && (
                    <Button
                      variant="outline"
                      size="sm"
                      className="min-w-[100px]"
                      onClick={(e) => {
                        e.stopPropagation();
                        cancelDownload(model.name);
                      }}
                    >
                      Cancel
                    </Button>
                  )}

                  {/* Error - Show Retry button */}
                  {isError && !modelIsDownloading && (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        className="min-w-[100px]"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          downloadModel(model.name);
                        }}
                      >
                        <RefreshCw className="mr-2 h-4 w-4" />
                        Retry
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          importModel(model.name);
                        }}
                      >
                        <FileUp className="mr-2 h-4 w-4" />
                        {modelIsImporting ? 'Importing...' : 'Import File'}
                      </Button>
                    </>
                  )}

                  {/* Corrupted - Show both Retry and Delete buttons */}
                  {isCorrupted && !modelIsDownloading && (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          downloadModel(model.name);
                        }}
                      >
                        <RefreshCw className="mr-2 h-4 w-4" />
                        Retry
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          deleteModel(model.name);
                        }}
                      >
                        <Trash2 className="mr-2 h-4 w-4" />
                        Delete
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={modelIsImporting}
                        onClick={(e) => {
                          e.stopPropagation();
                          importModel(model.name);
                        }}
                      >
                        <FileUp className="mr-2 h-4 w-4" />
                        {modelIsImporting ? 'Importing...' : 'Import File'}
                      </Button>
                    </>
                  )}

                  {/* Available - Show small trash icon (only if not currently selected) */}
                  {isAvailable && !modelIsDownloading && selectedModel !== model.name && (
                    <button
                      className="p-2 rounded hover:bg-gray-100 transition-colors text-gray-500 hover:text-red-600"
                      onClick={(e) => {
                        e.stopPropagation();
                        deleteModel(model.name);
                      }}
                      title="Delete model"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  )}
                </div>
              </div>

              {/* Download progress bar */}
              {modelIsDownloading && progress !== undefined && (
                <div className="mt-3 pt-3 border-t border-gray-200">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-sm font-medium text-gray-900">Downloading...</span>
                    <span className="text-sm font-semibold text-gray-900">
                      {Math.round(progress)}%
                    </span>
                  </div>
                  <div className="text-sm text-gray-600 mb-2">
                    {progressInfo?.totalMb > 0 ? (
                      <>
                        {progressInfo.downloadedMb.toFixed(1)} MB / {progressInfo.totalMb.toFixed(1)} MB
                        {progressInfo.speedMbps > 0 && (
                          <span className="ml-2 text-gray-500">
                            ({progressInfo.speedMbps.toFixed(1)} MB/s)
                          </span>
                        )}
                      </>
                    ) : (
                      <span>{model.size_mb} MB</span>
                    )}
                  </div>
                  <div className="w-full h-2.5 bg-gray-200 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-gradient-to-r from-gray-800 to-gray-900 rounded-full transition-all duration-300"
                      style={{ width: `${progress}%` }}
                    />
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

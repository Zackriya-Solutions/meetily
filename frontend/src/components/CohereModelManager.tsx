import React, { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { motion } from 'framer-motion';
import { toast } from 'sonner';
import {
  CohereAPI,
  CohereModelInfo,
  COHERE_MODEL_DISPLAY,
  DEFAULT_COHERE_MODEL,
  ModelStatus,
} from '../lib/cohere';

interface CohereModelManagerProps {
  selectedModel?: string;
  onModelSelect?: (modelName: string) => void;
  className?: string;
  autoSave?: boolean;
}

interface DownloadProgressPayload {
  modelName?: string;
  progress?: number;
  downloaded?: number;
  total?: number;
}

const formatMB = (mb: number) => (mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(0)} MB`);

const isAvailable = (status: ModelStatus) => status === 'Available';
const downloadingProgress = (status: ModelStatus): number | null => {
  if (typeof status === 'object' && status !== null && 'Downloading' in status) {
    return status.Downloading;
  }
  return null;
};

export function CohereModelManager({
  selectedModel,
  onModelSelect,
  className = '',
  autoSave = false,
}: CohereModelManagerProps) {
  const [models, setModels] = useState<CohereModelInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [activeModel, setActiveModel] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<Record<string, number>>({});

  const onModelSelectRef = useRef(onModelSelect);
  const autoSaveRef = useRef(autoSave);

  useEffect(() => {
    onModelSelectRef.current = onModelSelect;
    autoSaveRef.current = autoSave;
  }, [onModelSelect, autoSave]);

  const refresh = useCallback(async () => {
    try {
      const list = await CohereAPI.getAvailableModels();
      setModels(list);
      const current = await CohereAPI.getCurrentModel();
      setActiveModel(current);
    } catch (e) {
      console.error('Failed to refresh Cohere models', e);
    }
  }, []);

  useEffect(() => {
    if (initialized) return;

    const run = async () => {
      try {
        setLoading(true);
        await CohereAPI.init();
        await refresh();
        setInitialized(true);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        toast.error('Cohere 모델 초기화 실패', { description: msg, duration: 5000 });
      } finally {
        setLoading(false);
      }
    };

    run();
  }, [initialized, refresh]);

  // Subscribe to download events
  useEffect(() => {
    const unlistens: Array<() => void> = [];

    (async () => {
      unlistens.push(
        await listen<DownloadProgressPayload>('cohere-download-progress', (event) => {
          const { modelName, progress } = event.payload || {};
          if (!modelName || progress == null) return;
          setDownloading((prev) => ({ ...prev, [modelName]: progress }));
        }),
      );

      unlistens.push(
        await listen<DownloadProgressPayload>('cohere-download-complete', async (event) => {
          const name = event.payload?.modelName;
          setDownloading((prev) => {
            const next = { ...prev };
            if (name) delete next[name];
            return next;
          });
          toast.success(`Cohere 모델 다운로드 완료${name ? `: ${name}` : ''}`);
          await refresh();
        }),
      );

      unlistens.push(
        await listen<{ modelName?: string; error?: string }>('cohere-download-error', (event) => {
          const name = event.payload?.modelName;
          setDownloading((prev) => {
            const next = { ...prev };
            if (name) delete next[name];
            return next;
          });
          toast.error('Cohere 모델 다운로드 실패', {
            description: event.payload?.error ?? '알 수 없는 오류',
          });
        }),
      );
    })();

    return () => {
      unlistens.forEach((fn) => fn());
    };
  }, [refresh]);

  const startDownload = useCallback(async (name: string) => {
    try {
      setDownloading((prev) => ({ ...prev, [name]: 0 }));
      await CohereAPI.downloadModel(name);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error('다운로드를 시작하지 못했습니다', { description: msg });
      setDownloading((prev) => {
        const next = { ...prev };
        delete next[name];
        return next;
      });
    }
  }, []);

  const cancelDownload = useCallback(async () => {
    try {
      await CohereAPI.cancelDownload();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error('다운로드 취소 실패', { description: msg });
    }
  }, []);

  const selectModel = useCallback(async (name: string) => {
    try {
      await CohereAPI.loadModel(name);
      setActiveModel(name);
      onModelSelectRef.current?.(name);
      if (autoSaveRef.current) {
        toast.success(`Cohere 모델이 활성화되었습니다: ${name}`);
      }
      await refresh();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error('모델 로드 실패', { description: msg });
    }
  }, [refresh]);

  const placeholder = (name: string): CohereModelInfo => ({
    name,
    path: '',
    size_mb: 2048,
    status: 'Missing',
  });

  const modelList = models.length > 0 ? models : [placeholder(DEFAULT_COHERE_MODEL)];

  return (
    <div className={className}>
      <div className="mb-3 text-sm text-gray-600 dark:text-gray-300">
        Cohere Transcribe 03-2026 — 로컬 ONNX · 네트워크 전송 없음 · 한국어 기본
      </div>

      {error && (
        <div className="mb-3 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </div>
      )}

      {loading && (
        <div className="rounded-md border p-4 text-sm text-gray-500">모델 정보를 불러오는 중…</div>
      )}

      <div className="space-y-3">
        {modelList.map((model) => {
          const display = COHERE_MODEL_DISPLAY[model.name];
          const isCurrent = activeModel === model.name || selectedModel === model.name;
          const progress = downloading[model.name] ?? downloadingProgress(model.status);
          const available = isAvailable(model.status);

          return (
            <motion.div
              key={model.name}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              className={`rounded-lg border p-4 transition-colors ${
                isCurrent
                  ? 'border-blue-500 bg-blue-50 dark:bg-blue-950/40'
                  : 'border-gray-200 dark:border-gray-700'
              }`}
            >
              <div className="flex items-start justify-between">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="text-base font-semibold">
                      {display?.title ?? model.name}
                    </span>
                    {isCurrent && (
                      <span className="rounded-full bg-blue-600 px-2 py-0.5 text-xs font-medium text-white">
                        활성
                      </span>
                    )}
                  </div>
                  <div className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    {display?.tagline ?? 'Cohere 로컬 ONNX STT'}
                  </div>
                  <div className="mt-2 text-xs text-gray-500">
                    예상 크기: {display?.approxSizeGb ?? formatMB(model.size_mb)}
                  </div>
                </div>

                <div className="flex flex-col items-end gap-2">
                  {progress != null ? (
                    <>
                      <div className="text-xs text-gray-600 dark:text-gray-300">
                        다운로드 중 {Math.round(progress)}%
                      </div>
                      <button
                        className="rounded-md border border-gray-300 px-3 py-1 text-xs hover:bg-gray-100 dark:border-gray-600 dark:hover:bg-gray-800"
                        onClick={cancelDownload}
                      >
                        취소
                      </button>
                    </>
                  ) : available ? (
                    isCurrent ? (
                      <span className="text-xs text-gray-500">사용 중</span>
                    ) : (
                      <button
                        className="rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700"
                        onClick={() => selectModel(model.name)}
                      >
                        선택
                      </button>
                    )
                  ) : (
                    <button
                      className="rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700"
                      onClick={() => startDownload(model.name)}
                    >
                      다운로드
                    </button>
                  )}
                </div>
              </div>

              {progress != null && (
                <div className="mt-3 h-2 w-full overflow-hidden rounded-full bg-gray-200 dark:bg-gray-700">
                  <div
                    className="h-full bg-blue-600 transition-all"
                    style={{ width: `${Math.min(100, Math.max(0, progress))}%` }}
                  />
                </div>
              )}
            </motion.div>
          );
        })}
      </div>

      <div className="mt-4 flex justify-end">
        <button
          className="text-xs text-blue-600 hover:underline"
          onClick={() => CohereAPI.openModelsFolder().catch((e) => toast.error(String(e)))}
        >
          모델 폴더 열기
        </button>
      </div>
    </div>
  );
}

export default CohereModelManager;

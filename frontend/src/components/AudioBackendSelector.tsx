import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Info } from 'lucide-react';

export interface BackendInfo {
  id: string;
  name: string;
  description: string;
}

interface AudioBackendSelectorProps {
  currentBackend?: string;
  onBackendChange?: (backend: string) => void;
  disabled?: boolean;
}

export function AudioBackendSelector({
  currentBackend: propBackend,
  onBackendChange,
  disabled = false,
}: AudioBackendSelectorProps) {
  const [backends, setBackends] = useState<BackendInfo[]>([]);
  const [currentBackend, setCurrentBackend] = useState<string>('coreaudio');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showTooltip, setShowTooltip] = useState(false);

  // Load available backends and current selection
  useEffect(() => {
    const loadBackends = async () => {
      try {
        setLoading(true);
        setError(null);

        // Get backend info (includes name and description)
        const backendInfo = await invoke<BackendInfo[]>('get_audio_backend_info');
        setBackends(backendInfo);

        // Get current backend if not provided via props
        if (!propBackend) {
          const current = await invoke<string>('get_current_audio_backend');
          setCurrentBackend(current);
        } else {
          setCurrentBackend(propBackend);
        }
      } catch (err) {
        console.error('Failed to load audio backends:', err);
        setError('백엔드 옵션을 불러오지 못했습니다');
      } finally {
        setLoading(false);
      }
    };

    loadBackends();
  }, [propBackend]);

  // Handle backend selection
  const handleBackendChange = async (backendId: string) => {
    try {
      setError(null);
      await invoke('set_audio_backend', { backend: backendId });
      setCurrentBackend(backendId);

      // Notify parent component
      if (onBackendChange) {
        onBackendChange(backendId);
      }

      console.log(`Audio backend changed to: ${backendId}`);
    } catch (err) {
      console.error('Failed to set audio backend:', err);
      setError('백엔드를 변경하지 못했습니다. 다시 시도해 주세요.');
    }
  };

  // Only show selector if there are multiple backends
  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-gray-200 rounded w-32 mb-2"></div>
        <div className="h-10 bg-gray-200 rounded"></div>
      </div>
    );
  }

  // Hide if only one backend available
  if (backends.length <= 1) {
    return null;
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2">
        <label className="text-sm font-medium text-gray-700">
          시스템 오디오 백엔드
        </label>
        <div className="relative">
          <button
            type="button"
            onMouseEnter={() => setShowTooltip(true)}
            onMouseLeave={() => setShowTooltip(false)}
            className="text-gray-400 hover:text-gray-600 transition-colors"
          >
            <Info className="h-4 w-4" />
          </button>
          {showTooltip && (
            <div className="absolute z-10 left-6 top-0 w-64 p-3 text-xs bg-gray-900 text-white rounded-lg shadow-lg">
              <p className="font-semibold mb-1">오디오 캡처 방식:</p>
              <ul className="space-y-1">
                {backends.map((backend) => (
                  <li key={backend.id}>
                    <span className="font-medium">{backend.name}:</span> {backend.description}
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-gray-300">
                여러 백엔드를 시도하여 시스템에 가장 잘 맞는 것을 찾아보세요.
              </p>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div className="p-2 text-xs text-red-700 bg-red-50 border border-red-200 rounded-md">
          {error}
        </div>
      )}

      <div className="space-y-2">
        {backends.map((backend) => {
          // Disable Core Audio option
          const isCoreAudio = backend.id === 'screencapturekit';
          const isDisabled = disabled || isCoreAudio;

          return (
            <label
              key={backend.id}
              className={`flex items-start p-3 border rounded-lg transition-all ${
                currentBackend === backend.id
                  ? 'border-blue-500 bg-blue-50'
                  : 'border-gray-300 hover:border-gray-400 bg-white'
              } ${isDisabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
            >
              <input
                type="radio"
                name="audioBackend"
                value={backend.id}
                checked={currentBackend === backend.id}
                onChange={() => handleBackendChange(backend.id)}
                disabled={isDisabled}
                className="mt-1 h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300"
              />
              <div className="ml-3 flex-1">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium text-gray-900">
                    {backend.name}
                  </span>
                  {currentBackend === backend.id && (
                    <span className="text-xs font-medium text-blue-600 bg-blue-100 px-2 py-0.5 rounded">
                      사용 중
                    </span>
                  )}
                  {isCoreAudio && (
                    <span className="text-xs font-medium text-gray-500 bg-gray-100 px-2 py-0.5 rounded">
                      비활성화됨
                    </span>
                  )}
                </div>
                <p className="mt-1 text-xs text-gray-600">{backend.description}</p>
              </div>
            </label>
          );
        })}
      </div>

      <div className="text-xs text-gray-500 space-y-1">
        <p>• 백엔드 선택은 시스템 오디오 캡처에만 적용됩니다</p>
        <p>• 마이크는 항상 기본 방식을 사용합니다</p>
        <p>• 변경 사항은 새 녹음 세션부터 반영됩니다</p>
      </div>
    </div>
  );
}
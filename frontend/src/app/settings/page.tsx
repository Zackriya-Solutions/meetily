'use client';

import React, { useState, useEffect, Suspense } from 'react';
import { ArrowLeft, Settings2, Mic, Database as DatabaseIcon, SparkleIcon, Key, Calendar } from 'lucide-react';
import { useRouter, useSearchParams } from 'next/navigation';
import { TranscriptSettings, TranscriptModelProps } from '@/components/TranscriptSettings';
import { RecordingSettings } from '@/components/RecordingSettings';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { SummaryModelSettings } from '@/components/SummaryModelSettings';
import { PersonalKeysSettings } from '@/components/PersonalKeysSettings';
import { CalendarIntegrationSettings } from '@/components/CalendarIntegrationSettings';
import { authFetch } from '@/lib/api';

type SettingsTab = 'general' | 'recording' | 'Transcriptionmodels' | 'summaryModels' | 'personalKeys' | 'calendar';

function SettingsContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'localWhisper',
    model: 'large-v3',
    apiKey: null
  });

  useEffect(() => {
    const tabParam = searchParams.get('tab');
    if (tabParam) {
      const validTabs: SettingsTab[] = ['general', 'recording', 'Transcriptionmodels', 'summaryModels', 'personalKeys', 'calendar'];
      if (validTabs.includes(tabParam as SettingsTab)) {
        setActiveTab(tabParam as SettingsTab);
      }
    }
  }, [searchParams]);

  const tabs = [
    { id: 'general' as const, label: 'General', icon: <Settings2 className="w-4 h-4" /> },
    { id: 'recording' as const, label: 'Recordings', icon: <Mic className="w-4 h-4" /> },
    { id: 'Transcriptionmodels' as const, label: 'Transcription', icon: <DatabaseIcon className="w-4 h-4" /> },
    { id: 'summaryModels' as const, label: 'Summary', icon: <SparkleIcon className="w-4 h-4" /> },
    { id: 'personalKeys' as const, label: 'Personal Keys', icon: <Key className="w-4 h-4" /> },
    { id: 'calendar' as const, label: 'Calendar', icon: <Calendar className="w-4 h-4" /> }
  ];

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const response = await authFetch('/get-transcript-config');
        if (response.ok) {
          const config = await response.json();
          if (config) {
            setTranscriptModelConfig({
              provider: config.provider || 'localWhisper',
              model: config.model || 'large-v3',
              apiKey: config.apiKey || null
            });
          }
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, []);

  return (
    <div className="h-screen bg-gray-50 flex flex-col">
      {/* Fixed Header */}
      <div className="sticky top-0 z-10 bg-gray-50 border-b border-gray-200">
        <div className="max-w-6xl mx-auto px-8 py-6">
          <div className="flex items-center gap-4">
            <button
              onClick={() => router.back()}
              className="flex items-center gap-2 text-gray-600 hover:text-gray-900 transition-colors"
            >
              <ArrowLeft className="w-5 h-5" />
              <span>Back</span>
            </button>
            <h1 className="text-3xl font-bold">Settings</h1>
          </div>
        </div>
      </div>

      {/* Scrollable Content */}
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-6xl mx-auto p-8 pt-6">
          {/* Tabs */}
          <div className="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden">
            <div className="flex border-b border-gray-200 overflow-x-auto">
              {tabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`flex items-center gap-2 px-6 py-4 text-sm font-medium transition-colors whitespace-nowrap ${activeTab === tab.id
                    ? 'border-b-2 border-blue-600 text-blue-600 bg-blue-50'
                    : 'text-gray-600 hover:text-gray-900 hover:bg-gray-50'
                    }`}
                >
                  {tab.icon}
                  {tab.label}
                </button>
              ))}
            </div>

            {/* Tab Content */}
            <div className="p-6">
              {activeTab === 'general' && <PreferenceSettings />}
              {activeTab === 'recording' && <RecordingSettings />}
              {activeTab === 'Transcriptionmodels' && (
                <TranscriptSettings
                  transcriptModelConfig={transcriptModelConfig}
                  setTranscriptModelConfig={setTranscriptModelConfig}
                // onSave={handleSaveConfig}
                />
              )}
              {activeTab === 'summaryModels' && <SummaryModelSettings />}
              {activeTab === 'personalKeys' && <PersonalKeysSettings />}
              {activeTab === 'calendar' && <CalendarIntegrationSettings />}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function SettingsPage() {
  return (
    <Suspense fallback={<div className="h-screen bg-gray-50 flex items-center justify-center">Loading settings...</div>}>
      <SettingsContent />
    </Suspense>
  );
}

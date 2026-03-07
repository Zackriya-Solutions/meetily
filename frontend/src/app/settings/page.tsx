'use client';

import React, { useState, useEffect, useLayoutEffect, useRef } from 'react';
import { ArrowLeft, Settings2, Mic, Database as DatabaseIcon, SparkleIcon, Plug } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { motion } from 'framer-motion';
import { TranscriptSettings } from '@/components/TranscriptSettings';
import { RecordingSettings } from '@/components/RecordingSettings';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { SummaryModelSettings } from '@/components/SummaryModelSettings';
import { DiarizationSettings } from '@/components/DiarizationSettings';
import { OutlineSettings } from '@/components/OutlineSettings';
import { useConfig } from '@/contexts/ConfigContext';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';

// Tabs configuration (constant)
const TABS = [
  { value: 'general', label: 'General', icon: Settings2 },
  { value: 'recording', label: 'Recordings', icon: Mic },
  { value: 'Transcriptionmodels', label: 'Transcription', icon: DatabaseIcon },
  { value: 'summaryModels', label: 'Summary', icon: SparkleIcon },
  { value: 'integrations', label: 'Integrations', icon: Plug },
] as const;

export default function SettingsPage() {
  const router = useRouter();
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();

  // Animation state for tabs
  const [activeTab, setActiveTab] = useState('general');
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [underlineStyle, setUnderlineStyle] = useState({ left: 0, width: 0 });

  // Load saved transcript configuration on mount
  useEffect(() => {
    const loadTranscriptConfig = async () => {
      try {
        const config = await invoke('api_get_transcript_config') as any;
        if (config) {
          console.log('Loaded saved transcript config:', config);
          setTranscriptModelConfig({
            provider: config.provider || 'localWhisper',
            model: config.model || 'large-v3',
            apiKey: config.apiKey || null
          });
        }
      } catch (error) {
        console.error('Failed to load transcript config:', error);
      }
    };
    loadTranscriptConfig();
  }, [setTranscriptModelConfig]);

  // Update underline position when active tab changes
  useLayoutEffect(() => {
    const activeIndex = TABS.findIndex(tab => tab.value === activeTab);
    const activeTabElement = tabRefs.current[activeIndex];

    if (activeTabElement) {
      const { offsetLeft, offsetWidth } = activeTabElement;
      setUnderlineStyle({ left: offsetLeft, width: offsetWidth });
    }
  }, [activeTab]);

  return (
    <div className="h-screen bg-gray-50 dark:bg-gray-950 flex flex-col">
      {/* Fixed Header */}
      <div className="sticky top-0 z-10 bg-gray-50 dark:bg-gray-950 border-b border-gray-200 dark:border-gray-800">
        <div className="max-w-6xl mx-auto px-8 py-6">
          <div className="flex items-center gap-4">
            <button
              onClick={() => router.back()}
              className="flex items-center gap-2 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 transition-colors"
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
          <Tabs value={activeTab} onValueChange={setActiveTab}>
            <TabsList className="bg-transparent relative rounded-none border-b border-gray-200 dark:border-gray-700 p-0 h-auto">
              {TABS.map((tab, index) => {
                const Icon = tab.icon;
                return (
                  <TabsTrigger
                    key={tab.value}
                    value={tab.value}
                    ref={el => { tabRefs.current[index] = el }}
                    className="flex items-center gap-2 px-6 py-4 bg-transparent rounded-none border-0 data-[state=active]:bg-transparent data-[state=active]:shadow-none text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 relative z-10"
                    style={activeTab === tab.value ? { color: 'hsl(var(--theme-accent))' } : {}}
                  >
                    <Icon className="w-4 h-4" />
                    {tab.label}
                  </TabsTrigger>
                );
              })}

              <motion.div
                className="absolute bottom-0 z-20 h-0.5"
                style={{ backgroundColor: 'hsl(var(--theme-accent))' }}
                layoutId="underline"
                transition={{ type: 'spring', stiffness: 400, damping: 40 }}
                animate={{ left: underlineStyle.left, width: underlineStyle.width }}
              />
            </TabsList>

            <TabsContent value="general" className="mt-6">
              <PreferenceSettings />
            </TabsContent>
            <TabsContent value="recording">
              <div className="mt-6 p-6 border border-gray-200 dark:border-border rounded-lg bg-white dark:bg-card shadow-sm">
                <RecordingSettings />
              </div>
            </TabsContent>
            <TabsContent value="Transcriptionmodels">
              <div className="mt-6 space-y-4">
                <div className="p-6 border border-gray-200 dark:border-border rounded-lg bg-white dark:bg-card shadow-sm">
                  <h3 className="text-lg font-semibold text-gray-900 dark:text-foreground mb-1">Transcription Model</h3>
                  <p className="text-sm text-gray-600 dark:text-muted-foreground mb-6">Choose the engine used to transcribe your meetings.</p>
                  <TranscriptSettings
                    transcriptModelConfig={transcriptModelConfig}
                    setTranscriptModelConfig={setTranscriptModelConfig}
                  />
                </div>
                <div className="p-6 border border-gray-200 dark:border-border rounded-lg bg-white dark:bg-card shadow-sm">
                  <DiarizationSettings />
                </div>
              </div>
            </TabsContent>
            <TabsContent value="summaryModels" className="mt-6">
              <SummaryModelSettings />
            </TabsContent>
            <TabsContent value="integrations">
              <div className="mt-6 p-6 border border-gray-200 dark:border-border rounded-lg bg-white dark:bg-card shadow-sm">
                <OutlineSettings />
              </div>
            </TabsContent>
          </Tabs>
        </div>
      </div>
    </div>
  );
};

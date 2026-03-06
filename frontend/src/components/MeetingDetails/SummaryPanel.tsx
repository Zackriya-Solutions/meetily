"use client";

import { Summary, SummaryResponse, Transcript } from '@/types';
import { EditableTitle } from '@/components/EditableTitle';
import { BlockNoteSummaryView, BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';
import { EmptyStateSummary } from '@/components/EmptyStateSummary';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { SummaryGeneratorButtonGroup } from './SummaryGeneratorButtonGroup';
import { SummaryUpdaterButtonGroup } from './SummaryUpdaterButtonGroup';
import Analytics from '@/lib/analytics';
import { RefObject, useState } from 'react';
import { RefineNotesSidebar } from './RefineNotesSidebar';

import { Trash2, X } from 'lucide-react'; // Add Trash2 and X icon

interface SummaryPanelProps {
  meeting: {
    id: string;
    title: string;
    created_at: string;
  };
  meetingTitle: string;
  onTitleChange: (title: string) => void;
  isEditingTitle: boolean;
  onStartEditTitle: () => void;
  onFinishEditTitle: () => void;
  isTitleDirty: boolean;
  summaryRef: RefObject<BlockNoteSummaryViewRef>;
  isSaving: boolean;
  onSaveAll: () => Promise<void>;
  onCopySummary: () => Promise<void>;
  onOpenFolder?: () => Promise<void>;
  aiSummary: Summary | null;
  summaryStatus: 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';
  transcripts: Transcript[];
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSaveModelConfig: (config?: ModelConfig) => Promise<void>;
  onGenerateSummary: (customPrompt: string) => Promise<void>;
  summaryResponse: SummaryResponse | null;
  onSaveSummary: (summary: Summary | { markdown?: string; summary_json?: any[] }) => Promise<void>;
  onSummaryChange: (summary: Summary) => void;
  onDirtyChange: (isDirty: boolean) => void;
  summaryError: string | null;
  onRegenerateSummary: () => Promise<void>;
  onRegenerateWithDiarized?: () => Promise<void>;
  notesGenerationInfo?: {
    transcript_source?: string | null;
    diarized_available?: boolean;
    recommend_regenerate_with_diarized?: boolean;
  } | null;
  getSummaryStatusMessage: (status: 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error') => string;
  availableTemplates: Array<{ id: string, name: string, description: string }>;
  selectedTemplate: string;
  onTemplateSelect: (templateId: string, templateName: string) => void;
  isModelConfigLoading?: boolean;
  onDeleteMeeting: () => Promise<void>;
}

export function SummaryPanel({
  meeting,
  meetingTitle,
  onTitleChange,
  isEditingTitle,
  onStartEditTitle,
  onFinishEditTitle,
  isTitleDirty,
  summaryRef,
  isSaving,
  onSaveAll,
  onCopySummary,
  onOpenFolder,
  aiSummary,
  summaryStatus,
  transcripts,
  modelConfig,
  setModelConfig,
  onSaveModelConfig,
  onGenerateSummary,
  summaryResponse,
  onSaveSummary,
  onSummaryChange,
  onDirtyChange,
  summaryError,
  onRegenerateSummary,
  onRegenerateWithDiarized,
  notesGenerationInfo,
  getSummaryStatusMessage,
  availableTemplates,
  selectedTemplate,
  onTemplateSelect,
  isModelConfigLoading = false,
  onDeleteMeeting
}: SummaryPanelProps) {
  const isSummaryLoading = summaryStatus === 'processing' || summaryStatus === 'summarizing' || summaryStatus === 'regenerating';
  const [isRefineSidebarOpen, setIsRefineSidebarOpen] = useState(false);
  const [currentNotesContent, setCurrentNotesContent] = useState('');
  const [isDiarizedPromptDismissed, setIsDiarizedPromptDismissed] = useState(false);

  const handleOpenRefine = async () => {
    if (summaryRef.current) {
      const md = await summaryRef.current.getMarkdown();
      setCurrentNotesContent(md);
      setIsRefineSidebarOpen(true);
    }
  };

  const handleApplyRefinement = (newNotes: string) => {
    // Construct a summary object that simulates markdown format so BlockNoteView picks it up
    const updatedSummary = {
      markdown: newNotes
    } as any;

    onSummaryChange(updatedSummary);
    setIsRefineSidebarOpen(false);
  };


  return (
    <div className="flex-1 min-w-0 flex flex-col bg-white overflow-hidden">
      {/* Title area */}
      <div className="p-4 border-b border-gray-200 flex items-center justify-between">
        <div className="flex-1 mr-4">
          <EditableTitle
            title={meetingTitle}
            isEditing={isEditingTitle}
            onStartEditing={onStartEditTitle}
            onFinishEditing={onFinishEditTitle}
            onChange={onTitleChange}
          />
        </div>

        {/* Delete Button */}
        <button
          onClick={onDeleteMeeting}
          className="p-2 text-gray-500 hover:text-red-600 hover:bg-red-50 rounded-lg transition-colors"
          title="Delete meeting"
        >
          <Trash2 className="w-5 h-5" />
        </button>
      </div>

      {/* Button groups - only show when summary exists */}
      {aiSummary && !isSummaryLoading && (
        <div className="flex items-center justify-center w-full pt-4 gap-2">
          {/* Left-aligned: Summary Generator Button Group */}
          <div className="flex-shrink-0">
            <SummaryGeneratorButtonGroup
              modelConfig={modelConfig}
              setModelConfig={setModelConfig}
              onSaveModelConfig={onSaveModelConfig}
              onGenerateSummary={onGenerateSummary}
              summaryStatus={summaryStatus}
              availableTemplates={availableTemplates}
              selectedTemplate={selectedTemplate}
              onTemplateSelect={onTemplateSelect}
              hasTranscripts={transcripts.length > 0}
              isModelConfigLoading={isModelConfigLoading}
            />
          </div>

          {/* Right-aligned: Summary Updater Button Group */}
          <div className="flex-shrink-0">
            <SummaryUpdaterButtonGroup
              isSaving={isSaving}
              isDirty={isTitleDirty || (summaryRef.current?.isDirty || false)}
              onSave={onSaveAll}
              onCopy={onCopySummary}
              onFind={() => {
                // TODO: Implement find in summary functionality
                console.log('Find in summary clicked');
              }}
              onOpenFolder={onOpenFolder}
              onRefine={handleOpenRefine}
              hasSummary={!!aiSummary}
            />
          </div>
        </div>
      )}

      {aiSummary && !isSummaryLoading && notesGenerationInfo?.recommend_regenerate_with_diarized && !isDiarizedPromptDismissed && (
        <div className="mx-6 mt-4 p-3 rounded-lg border border-amber-200 bg-amber-50 flex items-center justify-between gap-3">
          <p className="text-sm text-amber-900">
            Speaker-aware transcript is available. Regenerating with diarized transcript can improve notes quality.
          </p>
          <div className="flex items-center gap-2">
            <button
              onClick={() => onRegenerateWithDiarized?.()}
              className="px-3 py-1.5 rounded-md bg-amber-600 text-white text-sm hover:bg-amber-700"
            >
              Regenerate with diarized
            </button>
            <button
              onClick={() => setIsDiarizedPromptDismissed(true)}
              className="p-1.5 rounded-md text-amber-600 hover:bg-amber-100 transition-colors"
              title="Dismiss"
            >
              <X size={16} />
            </button>
          </div>
        </div>
      )}


      {
        isSummaryLoading ? (
          <div className="flex flex-col h-full">
            {/* Show button group during generation */}
            <div className="flex items-center justify-center pt-8 pb-4">
              <SummaryGeneratorButtonGroup
                modelConfig={modelConfig}
                setModelConfig={setModelConfig}
                onSaveModelConfig={onSaveModelConfig}
                onGenerateSummary={onGenerateSummary}
                summaryStatus={summaryStatus}
                availableTemplates={availableTemplates}
                selectedTemplate={selectedTemplate}
                onTemplateSelect={onTemplateSelect}
                hasTranscripts={transcripts.length > 0}
                isModelConfigLoading={isModelConfigLoading}
                isDiarizedAvailable={transcripts.some((t) => t.source === 'diarized') || !!notesGenerationInfo?.diarized_available}
                onRegenerateWithDiarized={onRegenerateWithDiarized}
              />
            </div>
            {/* Loading spinner */}
            <div className="flex items-center justify-center flex-1">
              <div className="text-center">
                <div className="inline-block animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-blue-500 mb-4"></div>
                <p className="text-gray-600">Generating AI Summary...</p>
              </div>
            </div>
          </div>
        ) : !aiSummary ? (
          <div className="flex flex-col h-full">
            {/* Centered Summary Generator Button Group when no summary */}
            <div className="flex items-center justify-center pt-8 pb-4">
              <SummaryGeneratorButtonGroup
                modelConfig={modelConfig}
                setModelConfig={setModelConfig}
                onSaveModelConfig={onSaveModelConfig}
                onGenerateSummary={onGenerateSummary}
                summaryStatus={summaryStatus}
                availableTemplates={availableTemplates}
                selectedTemplate={selectedTemplate}
                onTemplateSelect={onTemplateSelect}
                hasTranscripts={transcripts.length > 0}
                isModelConfigLoading={isModelConfigLoading}
                isDiarizedAvailable={transcripts.some((t) => t.source === 'diarized') || !!notesGenerationInfo?.diarized_available}
                onRegenerateWithDiarized={onRegenerateWithDiarized}
              />
            </div>
            {/* Empty state message */}
            <EmptyStateSummary
              onGenerate={() => onGenerateSummary('')}
              hasModel={modelConfig.provider !== null && modelConfig.model !== null}
              isGenerating={isSummaryLoading}
            />
          </div>
        ) : transcripts?.length > 0 && (
          <div className="flex-1 overflow-y-auto min-h-0">
            {summaryResponse && (
              <div className="fixed bottom-0 left-0 right-0 bg-white shadow-lg p-4 max-h-1/3 overflow-y-auto">
                <h3 className="text-lg font-semibold mb-2">Meeting Summary</h3>
                <div className="grid grid-cols-2 gap-4">
                  <div className="bg-white p-4 rounded-lg shadow-sm">
                    <h4 className="font-medium mb-1">Key Points</h4>
                    <ul className="list-disc pl-4">
                      {summaryResponse.summary.key_points.blocks.map((block, i) => (
                        <li key={i} className="text-sm">{block.content}</li>
                      ))}
                    </ul>
                  </div>
                  <div className="bg-white p-4 rounded-lg shadow-sm mt-4">
                    <h4 className="font-medium mb-1">Action Items</h4>
                    <ul className="list-disc pl-4">
                      {summaryResponse.summary.action_items.blocks.map((block, i) => (
                        <li key={i} className="text-sm">{block.content}</li>
                      ))}
                    </ul>
                  </div>
                  <div className="bg-white p-4 rounded-lg shadow-sm mt-4">
                    <h4 className="font-medium mb-1">Decisions</h4>
                    <ul className="list-disc pl-4">
                      {summaryResponse.summary.decisions.blocks.map((block, i) => (
                        <li key={i} className="text-sm">{block.content}</li>
                      ))}
                    </ul>
                  </div>
                  <div className="bg-white p-4 rounded-lg shadow-sm mt-4">
                    <h4 className="font-medium mb-1">Main Topics</h4>
                    <ul className="list-disc pl-4">
                      {summaryResponse.summary.main_topics.blocks.map((block, i) => (
                        <li key={i} className="text-sm">{block.content}</li>
                      ))}
                    </ul>
                  </div>
                </div>
                {summaryResponse.raw_summary ? (
                  <div className="mt-4">
                    <h4 className="font-medium mb-1">Full Summary</h4>
                    <p className="text-sm whitespace-pre-wrap">{summaryResponse.raw_summary}</p>
                  </div>
                ) : null}
              </div>
            )}
            <div className="p-6 w-full">
              <BlockNoteSummaryView
                ref={summaryRef}
                summaryData={aiSummary}
                onSave={onSaveSummary}
                onSummaryChange={onSummaryChange}
                onDirtyChange={onDirtyChange}
                status={summaryStatus}
                error={summaryError}
                onRegenerateSummary={() => {
                  Analytics.trackButtonClick('regenerate_summary', 'meeting_details');
                  onRegenerateSummary();
                }}
                meeting={{
                  id: meeting.id,
                  title: meetingTitle,
                  created_at: meeting.created_at
                }}
              />
            </div>
            {summaryStatus !== 'idle' && (
              <div className={`mt-4 p-4 rounded-lg ${summaryStatus === 'error' ? 'bg-red-100 text-red-700' :
                summaryStatus === 'completed' ? 'bg-green-100 text-green-700' :
                  'bg-blue-100 text-blue-700'
                }`}>
                <p className="text-sm font-medium">{getSummaryStatusMessage(summaryStatus)}</p>
              </div>
            )}
          </div>
        )
      }

      {isRefineSidebarOpen && (
        <RefineNotesSidebar
          meetingId={meeting.id}
          onClose={() => setIsRefineSidebarOpen(false)}
          currentNotes={currentNotesContent}
          onApplyRefinement={handleApplyRefinement}
        />
      )}
    </div>
  );
}

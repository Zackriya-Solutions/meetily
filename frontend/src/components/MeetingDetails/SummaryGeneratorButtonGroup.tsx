"use client";

import { ModelConfig, ModelSettingsModal } from '@/components/ModelSettingsModal';
import {
  Dialog,
  DialogContent,
  DialogTrigger,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Sparkles, Settings, Loader2, FileText, Check, ChevronDown } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { toast } from 'sonner';
import { useState } from 'react';

interface SummaryGeneratorButtonGroupProps {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSaveModelConfig: (config?: ModelConfig) => Promise<void>;
  onGenerateSummary: (customPrompt: string) => Promise<void>;
  summaryStatus: 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';
  availableTemplates: Array<{ id: string, name: string, description: string }>;
  selectedTemplate: string;
  onTemplateSelect: (templateId: string, templateName: string) => void;
  hasTranscripts?: boolean;
  isModelConfigLoading?: boolean;
  isDiarizedAvailable?: boolean;
  onRegenerateWithDiarized?: () => Promise<void>;
}

export function SummaryGeneratorButtonGroup({
  modelConfig,
  setModelConfig,
  onSaveModelConfig,
  onGenerateSummary,
  summaryStatus,
  availableTemplates,
  selectedTemplate,
  onTemplateSelect,
  hasTranscripts = true,
  isModelConfigLoading = false,
  isDiarizedAvailable = false,
  onRegenerateWithDiarized
}: SummaryGeneratorButtonGroupProps) {
  const [isCheckingModels, setIsCheckingModels] = useState(false);
  const [settingsDialogOpen, setSettingsDialogOpen] = useState(false);

  if (!hasTranscripts) {
    return null;
  }

  const checkOllamaModelsAndGenerate = async () => {
    // Only check for Ollama provider
    // Also skip check if provider is gemini (redundant but safe)
    if (modelConfig.provider !== 'ollama') {
      onGenerateSummary('');
      return;
    }

    setIsCheckingModels(true);
    try {
      const endpoint = modelConfig.ollamaEndpoint || 'http://localhost:11434';

      // Use direct fetch to Ollama
      const response = await fetch(`${endpoint}/api/tags`);
      if (!response.ok) throw new Error('Failed to reach Ollama');

      const data = await response.json();
      const models = data.models || [];

      if (!models || models.length === 0) {
        // No models available, show message and open settings
        toast.error(
          'No Ollama models found. Please download gemma2:2b from Model Settings.',
          { duration: 5000 }
        );
        setSettingsDialogOpen(true);
        return;
      }

      // Models are available, proceed with generation
      onGenerateSummary('');
    } catch (error) {
      console.error('Error checking Ollama models:', error);
      toast.error(
        'Failed to check Ollama models. Please check if Ollama is running.',
        { duration: 5000 }
      );
      setSettingsDialogOpen(true);
    } finally {
      setIsCheckingModels(false);
    }
  };

  return (
    <ButtonGroup>
      {/* Generate Summary button or dropdown */}
      {onRegenerateWithDiarized ? (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="outline"
              size="sm"
              className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
              disabled={summaryStatus === 'processing' || isCheckingModels || isModelConfigLoading}
              title={
                isModelConfigLoading
                  ? 'Loading model configuration...'
                  : summaryStatus === 'processing'
                    ? 'Generating summary...'
                    : isCheckingModels
                      ? 'Checking models...'
                      : 'Generate AI Summary'
              }
            >
              {summaryStatus === 'processing' || isCheckingModels || isModelConfigLoading ? (
                <>
                  <Loader2 className="animate-spin xl:mr-2" size={18} />
                  <span className="hidden xl:inline">Processing...</span>
                </>
              ) : (
                <>
                  <Sparkles className="xl:mr-2" size={18} />
                  <span className="hidden lg:inline xl:inline">Generate Note</span>
                  <ChevronDown className="ml-2 h-4 w-4" />
                </>
              )}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem
              onClick={() => {
                Analytics.trackButtonClick('generate_summary', 'meeting_details');
                checkOllamaModelsAndGenerate();
              }}
            >
              Generate Note (Standard)
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!isDiarizedAvailable}
              onClick={() => {
                if (isDiarizedAvailable) {
                  Analytics.trackButtonClick('generate_summary_diarized', 'meeting_details');
                  onRegenerateWithDiarized();
                }
              }}
              title={!isDiarizedAvailable ? "Diarization must be completed first" : "Generate using speaker-aware transcript"}
            >
              Generate Note (Diarized)
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : (
        <Button
          variant="outline"
          size="sm"
          className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
          onClick={() => {
            Analytics.trackButtonClick('generate_summary', 'meeting_details');
            checkOllamaModelsAndGenerate();
          }}
          disabled={summaryStatus === 'processing' || isCheckingModels || isModelConfigLoading}
          title={
            isModelConfigLoading
              ? 'Loading model configuration...'
              : summaryStatus === 'processing'
                ? 'Generating summary...'
                : isCheckingModels
                  ? 'Checking models...'
                  : 'Generate AI Summary'
          }
        >
          {summaryStatus === 'processing' || isCheckingModels || isModelConfigLoading ? (
            <>
              <Loader2 className="animate-spin xl:mr-2" size={18} />
              <span className="hidden xl:inline">Processing...</span>
            </>
          ) : (
            <>
              <Sparkles className="xl:mr-2" size={18} />
              <span className="hidden lg:inline xl:inline">Generate Note</span>
            </>
          )}
        </Button>
      )}

      {/* Template selector dropdown */}



      {/* Template selector dropdown */}
      {availableTemplates.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="outline"
              size="sm"
              title="Select summary template"
            >
              <FileText />
              <span className="hidden lg:inline">Template</span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {availableTemplates.map((template) => (
              <DropdownMenuItem
                key={template.id}
                onClick={() => onTemplateSelect(template.id, template.name)}
                title={template.description}
                className="flex items-center justify-between gap-2"
              >
                <span>{template.name}</span>
                {selectedTemplate === template.id && (
                  <Check className="h-4 w-4 text-green-600" />
                )}
              </DropdownMenuItem>
            ))}

          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </ButtonGroup>
  );
}

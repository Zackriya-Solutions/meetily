'use client';

import { motion } from 'framer-motion';
import { FileQuestion, Sparkles, FileText, Check, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

interface EmptyStateSummaryProps {
  onGenerate: () => void;
  hasModel: boolean;
  isGenerating?: boolean;
  availableTemplates?: Array<{ id: string; name: string; description: string }>;
  selectedTemplate?: string;
  onTemplateSelect?: (templateId: string, templateName: string) => void;
}

export function EmptyStateSummary({
  onGenerate,
  hasModel,
  isGenerating = false,
  availableTemplates = [],
  selectedTemplate = '',
  onTemplateSelect,
}: EmptyStateSummaryProps) {
  const selectedTemplateName = availableTemplates.find((t) => t.id === selectedTemplate)?.name;

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col items-center justify-center h-full p-8 text-center"
    >
      <FileQuestion className="w-16 h-16 text-gray-300 mb-4" />
      <h3 className="text-lg font-semibold text-gray-900 mb-2">
        No Summary Generated Yet
      </h3>
      <p className="text-sm text-gray-500 mb-6 max-w-md">
        Generate an AI-powered summary of your meeting transcript to get key points, action items, and decisions.
      </p>

      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <div>
              <Button
                onClick={onGenerate}
                disabled={!hasModel || isGenerating}
                className="gap-2"
              >
                <Sparkles className="w-4 h-4" />
                {isGenerating ? 'Generating...' : 'Generate Summary'}
              </Button>
            </div>
          </TooltipTrigger>
          {!hasModel && (
            <TooltipContent>
              <p>Please select a model in Settings first</p>
            </TooltipContent>
          )}
        </Tooltip>
      </TooltipProvider>

      {availableTemplates.length > 0 && onTemplateSelect && (
        <div className="mt-4">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" className="gap-2 text-gray-600">
                <FileText className="w-4 h-4" />
                {selectedTemplateName ?? 'Select Template'}
                <ChevronDown className="w-3 h-3 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="center">
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
        </div>
      )}

      {!hasModel && (
        <p className="text-xs text-amber-600 mt-3">
          Please select a model in Settings first
        </p>
      )}
    </motion.div>
  );
}

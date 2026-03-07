"use client";

import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Copy, Save, Loader2, FileText, Upload, ChevronDown, BookOpen } from 'lucide-react';
import Analytics from '@/lib/analytics';

interface SummaryUpdaterButtonGroupProps {
  isSaving: boolean;
  isDirty: boolean;
  onSave: () => Promise<void>;
  onCopy: () => Promise<void>;
  onExportPdf: () => Promise<void>;
  onExportToOutline: () => Promise<void>;
  onFind?: () => void;
  hasSummary: boolean;
}

export function SummaryUpdaterButtonGroup({
  isSaving,
  isDirty,
  onSave,
  onCopy,
  onExportPdf,
  onExportToOutline,
  onFind,
  hasSummary
}: SummaryUpdaterButtonGroupProps) {
  return (
    <ButtonGroup>
      {/* Save button */}
      <Button
        variant="outline"
        size="sm"
        className={isDirty ? 'btn-accent' : ''}
        title={isSaving ? "Saving" : "Save Changes"}
        onClick={() => {
          Analytics.trackButtonClick('save_changes', 'meeting_details');
          onSave();
        }}
        disabled={isSaving}
      >
        {isSaving ? (
          <>
            <Loader2 className="animate-spin" />
            <span className="hidden lg:inline">Saving...</span>
          </>
        ) : (
          <>
            <Save />
            <span className="hidden lg:inline">Save</span>
          </>
        )}
      </Button>

      {/* Copy button */}
      <Button
        variant="outline"
        size="sm"
        title="Copy Summary"
        onClick={() => {
          Analytics.trackButtonClick('copy_summary', 'meeting_details');
          onCopy();
        }}
        disabled={!hasSummary}
        className="cursor-pointer"
      >
        <Copy />
        <span className="hidden lg:inline">Copy</span>
      </Button>

      {/* Export dropdown */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            disabled={!hasSummary}
            className="cursor-pointer gap-1"
            title="Export summary"
          >
            <Upload className="w-4 h-4" />
            <span className="hidden lg:inline">Export</span>
            <ChevronDown className="w-3 h-3 opacity-60" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem
            onClick={() => {
              Analytics.trackButtonClick('export_pdf', 'meeting_details');
              onExportPdf();
            }}
          >
            <FileText className="w-4 h-4 mr-2" />
            PDF
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={() => {
              Analytics.trackButtonClick('export_outline', 'meeting_details');
              onExportToOutline();
            }}
          >
            <BookOpen className="w-4 h-4 mr-2" />
            Outline (Notes)
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </ButtonGroup>
  );
}

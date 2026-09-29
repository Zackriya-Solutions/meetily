'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { FileText, PanelLeftOpen, Sparkles } from 'lucide-react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { PaneResizeHandle } from '@/components/PaneResizeHandle';
import { readStoredNumber, usePaneResize, writeStoredValue } from '@/hooks/usePaneResize';
const STORAGE_KEY = 'meetily.meetingDetails.transcriptPaneRatio';
const COLLAPSED_STORAGE_KEY = 'meetily.meetingDetails.transcriptPaneCollapsed';
const DEFAULT_RATIO = 0.3;
const MIN_RATIO = 0.3;
const MAX_RATIO = 0.5;
// Dragging the handle this far past the minimum and letting go collapses the transcript.
const COLLAPSE_BELOW_RATIO = 0.15;

const TABS = [
  { value: 'transcript' as const, label: 'Transcript', icon: FileText },
  { value: 'summary' as const, label: 'Summary', icon: Sparkles },
];

interface TranscriptPaneControls {
  collapse: () => void;
}

const TranscriptPaneContext = createContext<TranscriptPaneControls | null>(null);

/**
 * Controls for the transcript pane when it sits beside the summary (desktop
 * split view). Null in the stacked mobile layout and outside the split view.
 */
export function useTranscriptPaneControls(): TranscriptPaneControls | null {
  return useContext(TranscriptPaneContext);
}

export type MeetingDetailsTab = 'transcript' | 'summary';

interface MeetingDetailsSplitViewProps {
  transcript: ReactNode;
  summary: ReactNode;
  activeTab: MeetingDetailsTab;
  onTabChange: (tab: MeetingDetailsTab) => void;
}

export function MeetingDetailsSplitView({
  transcript,
  summary,
  activeTab,
  onTabChange,
}: MeetingDetailsSplitViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const handleRef = useRef<HTMLDivElement>(null);
  const expandButtonRef = useRef<HTMLButtonElement>(null);
  const focusAfterToggle = useRef(false);
  const [isDesktop, setIsDesktop] = useState(true);
  const [collapsed, setCollapsed] = useState(false);

  const setTranscriptCollapsed = useCallback((next: boolean) => {
    focusAfterToggle.current = true;
    setCollapsed(next);
    writeStoredValue(COLLAPSED_STORAGE_KEY, next ? '1' : '0');
  }, []);

  const { value: ratio, collapseArmed, handleProps } = usePaneResize({
    storageKey: STORAGE_KEY,
    defaultValue: DEFAULT_RATIO,
    min: MIN_RATIO,
    max: MAX_RATIO,
    step: 0.05,
    valueFromPointer: (clientX) => {
      const rect = containerRef.current?.getBoundingClientRect();
      return rect && rect.width > 0 ? (clientX - rect.left) / rect.width : null;
    },
    collapseBelow: COLLAPSE_BELOW_RATIO,
    onCollapse: () => setTranscriptCollapsed(true),
  });

  useEffect(() => {
    setCollapsed(readStoredNumber(COLLAPSED_STORAGE_KEY, 0, 1, 0) === 1);
  }, []);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(min-width: 768px)');
    const updateLayout = () => setIsDesktop(mediaQuery.matches);
    updateLayout();
    mediaQuery.addEventListener('change', updateLayout);
    return () => mediaQuery.removeEventListener('change', updateLayout);
  }, []);

  // Keep keyboard focus on a visible control when the transcript hides or returns.
  useEffect(() => {
    if (!focusAfterToggle.current) return;
    focusAfterToggle.current = false;
    (collapsed ? expandButtonRef.current : handleRef.current)?.focus();
  }, [collapsed]);

  const controls = useMemo<TranscriptPaneControls>(
    () => ({ collapse: () => setTranscriptCollapsed(true) }),
    [setTranscriptCollapsed],
  );
  const showCollapsed = isDesktop && collapsed;

  const transcriptPanelProps = isDesktop
    ? { role: 'region' as const, 'aria-label': 'Transcript', tabIndex: -1 }
    : {};
  const summaryPanelProps = isDesktop
    ? { role: 'region' as const, 'aria-label': 'Summary', tabIndex: -1 }
    : {};

  return (
    <Tabs
      value={activeTab}
      onValueChange={(value) => onTabChange(value as MeetingDetailsTab)}
      className="flex flex-1 min-h-0 min-w-0 flex-col overflow-hidden"
    >
      <div className="shrink-0 bg-white px-2 md:hidden">
        <TabsList className="relative h-auto w-full justify-center rounded-none border-b border-gray-200 bg-transparent p-0">
          {TABS.map((tab) => {
            const Icon = tab.icon;
            return (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className="relative z-10 flex items-center gap-2 rounded-none border-0 bg-transparent px-6 py-4 text-gray-600 data-[state=active]:bg-transparent data-[state=active]:text-blue-600 data-[state=active]:shadow-none hover:text-gray-900"
              >
                <Icon className="h-4 w-4" />
                {tab.label}
              </TabsTrigger>
            );
          })}
        </TabsList>
      </div>
      <div
        ref={containerRef}
        className="flex flex-1 min-h-0 min-w-0 flex-col md:flex-row"
        style={{ '--transcript-pane-width': `${ratio * 100}%` } as CSSProperties}
      >
        {showCollapsed && (
          <button
            ref={expandButtonRef}
            type="button"
            onClick={() => setTranscriptCollapsed(false)}
            aria-label="Show transcript"
            aria-expanded={false}
            title="Show transcript"
            className="hidden w-10 flex-shrink-0 flex-col items-center gap-3 border-r border-border bg-background py-4 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring md:flex"
          >
            <PanelLeftOpen className="h-4 w-4" />
            <span className="text-xs font-medium [writing-mode:vertical-rl]">Transcript</span>
          </button>
        )}
        <TabsContent
          value="transcript"
          forceMount
          className={`mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:w-[var(--transcript-pane-width)] md:flex-none md:transition-opacity ${
            showCollapsed ? 'md:hidden' : 'md:data-[state=inactive]:flex'
          } ${collapseArmed ? 'md:opacity-40' : ''}`}
          {...transcriptPanelProps}
        >
          <TranscriptPaneContext.Provider value={isDesktop ? controls : null}>
            {transcript}
          </TranscriptPaneContext.Provider>
        </TabsContent>
        {!showCollapsed && (
          <PaneResizeHandle
            ref={handleRef}
            handleProps={handleProps}
            label="Resize transcript and summary"
            valueNow={Math.round(ratio * 100)}
            valueMin={Math.round(MIN_RATIO * 100)}
            valueMax={Math.round(MAX_RATIO * 100)}
            valueText={
              collapseArmed
                ? 'Release to hide the transcript'
                : `Transcript panel ${Math.round(ratio * 100)} percent`
            }
            className="relative hidden md:flex"
          />
        )}
        <TabsContent
          value="summary"
          forceMount
          className="mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:data-[state=inactive]:flex"
          {...summaryPanelProps}
        >
          {summary}
        </TabsContent>
      </div>
    </Tabs>
  );
}

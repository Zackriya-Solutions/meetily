import type { SidebarItem } from './SidebarProvider';

/** Which pipeline stages a meeting has finished, as reported by `api_get_meetings`. */
export interface MeetingStages {
  transcribed: boolean;
  /**
   * A recording is still waiting to be transcribed. False for a meeting that
   * lacks a transcript but has no audio to make one from, such as a Teams recap.
   */
  transcriptNeeded: boolean;
  summarized: boolean;
  /** The meeting has a note in the Obsidian vault. */
  obsidianExported: boolean;
}

export type MeetingStage = 'transcribed' | 'summarized' | 'obsidianExported';

/** Sidebar filter chips, in display order: each shows meetings missing that stage. */
export const MISSING_STAGE_FILTERS: ReadonlyArray<{ stage: MeetingStage; label: string }> = [
  { stage: 'transcribed', label: 'Transcript needed' },
  { stage: 'summarized', label: 'No summary' },
  { stage: 'obsidianExported', label: 'Not in Obsidian' },
];

/** Whether `stage` is outstanding work for the meeting, as the filter chips count it. */
export function isMissingStage(meeting: Partial<MeetingStages>, stage: MeetingStage): boolean {
  return stage === 'transcribed' ? Boolean(meeting.transcriptNeeded) : !meeting[stage];
}

/** Keeps only the meetings that have not finished `stage`; folders stay. */
export function missingStage(items: SidebarItem[], stage: MeetingStage | null): SidebarItem[] {
  if (!stage) return items;
  return items.map(item =>
    item.children ? { ...item, children: item.children.filter(child => isMissingStage(child, stage)) } : item
  );
}

'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Loader2, Pause, Play, Square, Trash2, LogIn } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ConfirmationModal } from '@/components/ConfirmationModel/confirmation-modal';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

type PipelineStage = 'transcribe' | 'summarize';

interface PendingSnapshotItem {
  meeting_id: string;
  title: string;
  stage: PipelineStage;
  created_at: string;
  attempts: number;
  last_error: string | null;
  suppressed: boolean;
  eligible: boolean;
  /** The user clicked "Process now": runs next, even while paused. */
  queued: boolean;
}

interface CurrentItem {
  meeting_id: string;
  title: string;
  stage: string;
  started_at: string;
}

interface PipelineStatus {
  state: 'running' | 'paused' | 'auth_required';
  detail: string | null;
  enabled: boolean;
  current: CurrentItem | null;
  pending: PendingSnapshotItem[];
  last_scan_at: string | null;
}

/** Payload of `retranscription-progress` (audio/retranscription.rs). */
interface TranscriptionProgress {
  meeting_id: string;
  stage: string;
  progress_percentage: number;
  message: string;
}

const STAGE_LABEL: Record<PipelineStage, string> = {
  transcribe: 'Needs transcript',
  summarize: 'Needs summary',
};

const STAGE_BADGE: Record<PipelineStage, string> = {
  transcribe: 'bg-blue-50 text-blue-700',
  summarize: 'bg-amber-50 text-amber-700',
};

/**
 * Home-screen view of the automatic pipeline: what still needs transcribing
 * or summarizing, what it is working on right now, and why it is waiting.
 *
 * The processing itself lives in the Rust orchestrator
 * (`src-tauri/src/pipeline`), which keeps running with the window closed —
 * this panel only observes `pipeline-status` and `retranscription-progress`
 * events and offers the manual overrides: process a meeting now (skipping
 * the idle wait and a pause), pause/resume, stop (cancel the meeting in
 * progress and empty the queue), sign in to SharePoint, and remove a meeting
 * that keeps failing.
 */
export function PendingWorkPanel() {
  const { refetchMeetings } = useSidebar();
  const [status, setStatus] = useState<PipelineStatus | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [deleteTarget, setDeleteTarget] = useState<PendingSnapshotItem | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<TranscriptionProgress | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<PipelineStatus>('pipeline_get_status'));
    } catch (error) {
      console.error('Failed to read pipeline status:', error);
    }
  }, []);

  // The orchestrator pushes a status event on every state change, so the
  // panel never polls.
  useEffect(() => {
    void refresh();
    const unlisten = listen<PipelineStatus>('pipeline-status', (event) => {
      setStatus(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  // Live percentage for the transcription in flight (also fired by
  // user-initiated retranscriptions; rendering filters to the current item).
  useEffect(() => {
    const unlisten = listen<TranscriptionProgress>('retranscription-progress', (event) => {
      setProgress(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Drop stale progress once the pipeline moves off that meeting.
  useEffect(() => {
    const activeId = status?.current?.meeting_id ?? null;
    setProgress((prev) => (prev && prev.meeting_id === activeId ? prev : null));
  }, [status]);

  // A completed meeting leaves the pending list; keep the sidebar in step.
  useEffect(() => {
    if (!status) return;
    setSelected((prev) => {
      const alive = new Set(status.pending.map((item) => item.meeting_id));
      const next = new Set([...prev].filter((id) => alive.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [status]);

  const items = status?.pending ?? [];
  const current = status?.current ?? null;
  // List order is the order the loop picks work up in, so the index among
  // queued items is an honest queue position.
  const queuedIds = items.filter((i) => i.queued).map((i) => i.meeting_id);

  const toggleItem = useCallback((meetingId: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(meetingId) ? next.delete(meetingId) : next.add(meetingId);
      return next;
    });
  }, []);

  const allSelected = items.length > 0 && items.every((i) => selected.has(i.meeting_id));
  const toggleAll = useCallback(() => {
    setSelected(allSelected ? new Set() : new Set(items.map((i) => i.meeting_id)));
  }, [allSelected, items]);

  const handleProcessNow = useCallback(async () => {
    const meetingIds = [...selected];
    if (!meetingIds.length) return;
    setBusy(true);
    try {
      await invoke('pipeline_process_now', { meetingIds });
      setSelected(new Set());
      toast.success(
        meetingIds.length === 1
          ? 'Processing now'
          : `Processing ${meetingIds.length} meetings now`,
        status?.state === 'paused'
          ? { description: 'The pipeline stays paused for other meetings' }
          : undefined
      );
    } catch (error) {
      toast.error('Could not start processing', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setBusy(false);
    }
  }, [selected, status]);

  const handlePauseResume = useCallback(async () => {
    if (!status) return;
    setBusy(true);
    try {
      await invoke(status.state === 'paused' ? 'pipeline_resume' : 'pipeline_pause');
    } catch (error) {
      toast.error('Could not change the pipeline state', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setBusy(false);
    }
  }, [status]);

  const handleStop = useCallback(async () => {
    setBusy(true);
    try {
      await invoke('pipeline_stop');
      toast.success('Stopped', {
        description: 'The meetings stay in the list. Resume or Process now picks them up again.',
      });
    } catch (error) {
      toast.error('Could not stop the pipeline', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setBusy(false);
    }
  }, []);

  const handleSignIn = useCallback(async () => {
    setBusy(true);
    try {
      await invoke('pipeline_sign_in_to_sharepoint');
      toast.success('Signed in to SharePoint — importing resumed');
    } catch (error) {
      toast.error('SharePoint sign-in failed', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setBusy(false);
    }
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!deleteTarget) return;
    const meetingId = deleteTarget.meeting_id;
    setDeleteTarget(null);
    try {
      await invoke('api_delete_meeting', { meetingId });
      setSelected((prev) => {
        const next = new Set(prev);
        next.delete(meetingId);
        return next;
      });
      await refresh();
      await refetchMeetings();
      toast.success('Meeting deleted');
    } catch (error) {
      toast.error('Failed to delete meeting', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  }, [deleteTarget, refresh, refetchMeetings]);

  // Nothing outstanding and nothing to report: stay out of the way.
  if (!status || (items.length === 0 && !current && status.state === 'running')) {
    return null;
  }

  const stateBanner =
    status.state === 'auth_required'
      ? status.detail ?? 'SharePoint sign-in needed'
      : status.state === 'paused'
        ? 'Pipeline paused — only meetings you queue with Process now will run'
        : null;

  return (
    <div className="flex justify-center px-4 pt-4">
      <div className="w-2/3 max-w-[750px]">
        <p className="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">
          Pending work
        </p>
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-3">
          {stateBanner && (
            <div className="flex items-center justify-between gap-2 mb-2 px-2 py-1.5 rounded bg-amber-50 text-[12px] text-amber-800">
              <span className="truncate">{stateBanner}</span>
              {status.state === 'auth_required' && (
                <Button size="sm" variant="outline" onClick={handleSignIn} disabled={busy}>
                  <LogIn className="w-3.5 h-3.5" />
                  Sign in
                </Button>
              )}
            </div>
          )}

          {items.length > 1 && (
            <label className="flex items-center gap-2 pb-2 mb-1 border-b border-gray-100 text-xs text-gray-500 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={toggleAll}
                className="h-4 w-4 rounded border-gray-300 accent-gray-900"
              />
              Select all
            </label>
          )}

          <div className="space-y-1">
            {items.map((item) => {
              const isCurrent = current?.meeting_id === item.meeting_id;
              const rowProgress =
                isCurrent && progress?.meeting_id === item.meeting_id ? progress : null;
              return (
                <label
                  key={item.meeting_id}
                  className="flex items-center gap-2 py-1.5 cursor-pointer select-none"
                >
                  <input
                    type="checkbox"
                    checked={selected.has(item.meeting_id)}
                    onChange={() => toggleItem(item.meeting_id)}
                    disabled={isCurrent}
                    className="h-4 w-4 rounded border-gray-300 accent-gray-900"
                  />
                  <span className="flex-1 min-w-0">
                    <span className="block text-sm text-gray-900 truncate">{item.title}</span>
                    {rowProgress && (
                      <span className="flex items-center gap-2 mt-1">
                        <span className="flex-1 h-1 rounded-full bg-gray-200 overflow-hidden">
                          <span
                            className="block h-full bg-blue-500 transition-all duration-300"
                            style={{
                              width: `${Math.min(rowProgress.progress_percentage, 100)}%`,
                            }}
                          />
                        </span>
                        <span className="flex-shrink-0 text-[11px] text-gray-500 tabular-nums">
                          {rowProgress.progress_percentage}%
                        </span>
                      </span>
                    )}
                    {rowProgress && (
                      <span className="block text-[11px] text-gray-500 truncate">
                        {rowProgress.message}
                      </span>
                    )}
                    {!isCurrent && item.last_error && (
                      <span
                        className="block text-[11px] text-red-600 truncate"
                        title={item.last_error}
                      >
                        {item.last_error}
                      </span>
                    )}
                  </span>

                  {isCurrent ? (
                    <span className="flex-shrink-0 flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full bg-blue-50 text-blue-700">
                      <Loader2 className="w-3 h-3 animate-spin" />
                      {current?.stage === 'transcribe' ? 'Transcribing' : 'Summarizing'}
                    </span>
                  ) : item.suppressed ? (
                    <span className="flex-shrink-0 text-[11px] font-medium px-2 py-0.5 rounded-full bg-red-50 text-red-700">
                      Failed
                    </span>
                  ) : item.last_error ? (
                    <span
                      className="flex-shrink-0 text-[11px] font-medium px-2 py-0.5 rounded-full bg-orange-50 text-orange-700"
                      title={`Attempt ${item.attempts} failed; will retry`}
                    >
                      Retrying
                    </span>
                  ) : item.queued ? (
                    <span
                      className="flex-shrink-0 text-[11px] font-medium px-2 py-0.5 rounded-full bg-indigo-50 text-indigo-700"
                      title="Queued with Process now — runs next, even while paused"
                    >
                      {queuedIds.length > 1
                        ? `Queued #${queuedIds.indexOf(item.meeting_id) + 1}`
                        : 'Queued'}
                    </span>
                  ) : (
                    <span
                      className={`flex-shrink-0 text-[11px] font-medium px-2 py-0.5 rounded-full ${STAGE_BADGE[item.stage]}`}
                    >
                      {STAGE_LABEL[item.stage]}
                    </span>
                  )}

                  <span className="flex-shrink-0 text-xs text-gray-400">
                    {new Date(item.created_at).toLocaleDateString()}
                  </span>

                  {(item.suppressed || item.last_error) && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setDeleteTarget(item);
                      }}
                      disabled={isCurrent}
                      className="flex-shrink-0 p-1 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded transition-colors disabled:opacity-50"
                      title="Remove meeting"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  )}
                </label>
              );
            })}
          </div>

          <div className="flex items-center justify-between pt-2 mt-1 border-t border-gray-100">
            <Button size="sm" variant="ghost" onClick={handlePauseResume} disabled={busy}>
              {status.state === 'paused' ? (
                <>
                  <Play className="w-4 h-4" />
                  Resume
                </>
              ) : (
                <>
                  <Pause className="w-4 h-4" />
                  Pause
                </>
              )}
            </Button>
            {(current || queuedIds.length > 0) && (
              <Button
                size="sm"
                variant="ghost"
                onClick={handleStop}
                disabled={busy}
                className="mr-auto text-red-600 hover:text-red-700 hover:bg-red-50"
                title="Cancel the meeting in progress, empty the queue and pause the pipeline"
              >
                <Square className="w-4 h-4" />
                Stop
              </Button>
            )}
            <Button size="sm" onClick={handleProcessNow} disabled={selected.size === 0 || busy}>
              {`Process now${selected.size > 0 ? ` (${selected.size})` : ''}`}
            </Button>
          </div>
        </div>
      </div>
      <ConfirmationModal
        isOpen={deleteTarget !== null}
        text="Are you sure you want to delete this meeting? This action cannot be undone."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  );
}

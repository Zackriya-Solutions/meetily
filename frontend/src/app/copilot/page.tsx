'use client';

/**
 * `/copilot` — the route the panel window loads (BACKLOG I1, ADR-0038).
 *
 * A second OS window created by `copilot::window` opens this route from the
 * same static export as the main window. `app/layout.tsx` keeps the application
 * shell off it: no sidebar, no onboarding check, no update checker, no
 * drag-to-import listeners.
 *
 * The wiring here is deliberately thin — subscribe, poll, forward — because
 * everything this panel is allowed to do in I1 is display state that already
 * exists. In particular:
 *
 * - **No capture is started here.** There is no start-recording control; the
 *   panel reads the `transcript-update` stream of a session the user already
 *   consented to (the Rust consent ticket, `recording_consent.rs`).
 * - **Stopping is not duplicated.** Pause and resume are single, idempotent
 *   backend commands — the tray already calls exactly these. Stopping is not:
 *   it needs a save path plus the post-processing chain that lives in the main
 *   window's `useRecordingStop`, and `stop_recording` itself returns a boolean
 *   meaning "another caller owns shutdown". A second owner of that path is a
 *   corrupted-recording bug waiting to happen, so the panel sends the user to
 *   the main window instead.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { copilotService } from '@/services/copilotService';
import { recordingService } from '@/services/recordingService';
import { CopilotPanel, appendLine, PanelLine } from '@/components/copilot/CopilotPanel';
import type { InsightState } from '@/components/copilot/CopilotInsights';
import { isInsightFailure, type LiveAction } from '@/types/copilot';
import type { CopilotStatus } from '@/types/copilot';
import type { TranscriptUpdate } from '@/types';
import { isTauri } from '@/lib/isTauri';

/** How often the panel re-reads recording/protection state. */
const STATUS_POLL_MS = 4000;

export default function CopilotRoute() {
  const [status, setStatus] = useState<CopilotStatus | null>(null);
  const [lines, setLines] = useState<PanelLine[]>([]);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [insight, setInsight] = useState<InsightState>({ phase: 'idle' });
  /**
   * Which request the visible state belongs to.
   *
   * A cancelled or superseded request still resolves, and without this its
   * answer would land in the card after the user asked for something else. The
   * counter is the only thing that decides whether a result is still wanted.
   */
  const insightRequest = useRef(0);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const next = await copilotService.getStatus();
      if (!mounted.current) return;
      setStatus(next);
      // A finished session must not leave its last lines on screen: the panel
      // would then show a transcript with no recording behind it.
      if (!next.recording) setLines([]);
    } catch (e) {
      if (mounted.current) setError(String(e));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => void refresh(), STATUS_POLL_MS);
    return () => {
      mounted.current = false;
      clearInterval(timer);
    };
  }, [refresh]);

  useEffect(() => {
    if (!isTauri()) return;
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    const subscribe = async () => {
      const onTranscript = await listen<TranscriptUpdate>('transcript-update', (event) => {
        setLines((current) => appendLine(current, event.payload));
      });
      if (cancelled) {
        onTranscript();
        return;
      }
      unlisteners.push(onTranscript);

      // Two stop paths, two events: the ordinary stop emits `recording-stopped`
      // (`audio/recording_commands.rs`); the tray's stop emits
      // `recording-stop-complete` (`tray.rs`) and nothing else does. I1 listened
      // only to the second, so after a normal stop the last lines stayed on
      // screen until the status poll noticed — up to STATUS_POLL_MS later.
      for (const stopEvent of ['recording-stopped', 'recording-stop-complete'] as const) {
        const onStopped = await listen(stopEvent, () => {
          setLines([]);
          void refresh();
        });
        if (cancelled) {
          onStopped();
          return;
        }
        unlisteners.push(onStopped);
      }
    };

    void subscribe();
    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [refresh]);

  // Pause state is owned by the backend; the tray can change it too.
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const read = async () => {
      try {
        const isPaused = await invoke<boolean>('is_recording_paused');
        if (!cancelled) setPaused(isPaused);
      } catch {
        // Not recording, or the command is unavailable — neither is an error
        // worth showing in a panel this small.
      }
    };
    void read();
    const timer = setInterval(() => void read(), STATUS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [status?.recording]);

  const handleClose = useCallback(async () => {
    try {
      await copilotService.closePanel();
    } catch {
      // Fall back to closing our own window if the command is unavailable.
      await getCurrentWindow().close();
    }
  }, []);

  const handlePause = useCallback(async () => {
    // Each action clears the line it owns before retrying, so a failure that has
    // since been fixed does not sit under a successful one. The status poll
    // (every STATUS_POLL_MS) must never do this: it would wipe the message a few
    // seconds after the user caused it, before it could be read.
    setError(null);
    try {
      await recordingService.pauseRecording();
      setPaused(true);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleResume = useCallback(async () => {
    setError(null);
    try {
      await recordingService.resumeRecording();
      setPaused(false);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleRequestInsight = useCallback(async (action: LiveAction) => {
    const request = (insightRequest.current += 1);
    setInsight({ phase: 'loading', action });
    try {
      const outcome = await copilotService.requestInsight(action);
      if (insightRequest.current !== request) return;
      setInsight({ phase: 'done', outcome });
    } catch (e) {
      if (insightRequest.current !== request) return;
      // The backend's tagged failure is what the card branches on. A thrown
      // value that is not one is a bug in the bridge, not a refusal, so it is
      // shown as a provider failure rather than silently swallowed.
      setInsight({
        phase: 'failed',
        action,
        failure: isInsightFailure(e) ? e : { kind: 'provider', message: String(e) },
      });
    }
  }, []);

  const handleCancelInsight = useCallback(async () => {
    // Bumping first makes the in-flight result unwanted even if the backend
    // answers before the cancel lands.
    insightRequest.current += 1;
    setInsight({ phase: 'idle' });
    try {
      await copilotService.cancelInsight();
    } catch {
      // Cancelling is best-effort: the card is already back to idle, and an
      // error here would be noise about work the user has abandoned.
    }
  }, []);

  const handleOpenMainWindow = useCallback(async () => {
    setError(null);
    try {
      await copilotService.focusMainWindow();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return (
    // The panel component fills its container; the window is the container.
    <div className="h-screen w-screen overflow-hidden">
      <CopilotPanel
        status={status}
        lines={lines}
        paused={paused}
        error={error}
        onClose={handleClose}
        onPause={handlePause}
        onResume={handleResume}
        onOpenMainWindow={handleOpenMainWindow}
        insight={insight}
        onRequestInsight={handleRequestInsight}
        onCancelInsight={handleCancelInsight}
      />
    </div>
  );
}

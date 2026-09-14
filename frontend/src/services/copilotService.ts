/**
 * Copilot Service (BACKLOG I1, ADR-0038)
 *
 * The only place the copilot's Tauri commands are named — no raw `invoke` in
 * components (docs/CONVENTIONS.md).
 *
 * Outside the Tauri shell (the `/design/*` review routes, a browser preview)
 * `invoke` would throw, so every read resolves to a disabled stub and every
 * write is refused. That keeps the design fixture renderable without pretending
 * a backend answered.
 */

import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '@/lib/isTauri';
import type {
  CopilotConfig,
  CopilotStatus,
  LiveAction,
  LiveInsightOutcome,
} from '@/types/copilot';

/**
 * What the design routes see. Mirrors the Rust defaults: off, protection
 * requested, shortcuts on, and the platform verdict left deliberately vague
 * because no platform answered.
 */
const BROWSER_STUB: CopilotStatus = {
  config: {
    enabled: false,
    contentProtection: true,
    shortcutsEnabled: true,
    keybinds: {
      togglePanel: 'CommandOrControl+Shift+M',
      ask: 'CommandOrControl+Shift+A',
      captureScreen: 'CommandOrControl+Shift+S',
    },
    liveWindowSecs: 180,
    allowCloudInsights: false,
  },
  protection: {
    level: 'unsupported',
    headline: 'Screen-sharing behaviour unknown',
    detail: 'Preview only — no desktop app is running, so the platform has not been asked.',
  },
  panelOpen: false,
  recording: false,
  shortcuts: [],
  liveContext: {
    subscribed: false,
    windowSecs: 0,
    turns: 0,
    windowTurns: 0,
    evicted: 0,
  },
  liveActions: [],
};

export class CopilotService {
  /** Settings, platform verdict, panel and recording state in one read. */
  async getStatus(): Promise<CopilotStatus> {
    if (!isTauri()) return structuredClone(BROWSER_STUB);
    return invoke<CopilotStatus>('copilot_get_status');
  }

  /**
   * Save settings and apply them to the running app.
   *
   * @throws A human sentence when a shortcut is invalid — the backend
   * validates every binding before storing any of them, so a rejected save
   * changes nothing.
   */
  async setConfig(config: CopilotConfig): Promise<CopilotStatus> {
    if (!isTauri()) return structuredClone(BROWSER_STUB);
    return invoke<CopilotStatus>('copilot_set_config', { config });
  }

  /** Show or hide the panel — the same action as the global shortcut. */
  async togglePanel(): Promise<void> {
    if (!isTauri()) return;
    await invoke('copilot_toggle_panel');
  }

  /** Close the panel (its own close button). */
  async closePanel(): Promise<void> {
    if (!isTauri()) return;
    await invoke('copilot_close_panel');
  }

  /**
   * Bring the main window forward. The panel uses this instead of duplicating
   * controls that belong to the main window — stopping a recording above all.
   */
  async focusMainWindow(): Promise<void> {
    if (!isTauri()) return;
    await invoke('copilot_focus_main_window');
  }

  /**
   * Ask for one insight about the last few minutes of speech.
   *
   * **Rejects with an `InsightFailure`, not an `Error`.** The backend returns a
   * tagged failure so the panel can branch on `kind` — "this workspace keeps
   * live insights on the device" is a different thing to show than "the model
   * timed out", and matching on prose would break the first time a sentence is
   * reworded. Callers should use `isInsightFailure` rather than reading
   * `String(e)`.
   *
   * Outside Tauri there is no backend and no live window, so the honest answer
   * is the refusal — never a fabricated card in the design route.
   */
  async requestInsight(action: LiveAction): Promise<LiveInsightOutcome> {
    if (!isTauri()) return { status: 'noContext' };
    return invoke<LiveInsightOutcome>('copilot_request_insight', { action });
  }

  /**
   * Abandon the in-flight insight. Idempotent: safe to call with nothing
   * running, and the abandoned request resolves as `cancelled` rather than as
   * an error the user has to read.
   */
  async cancelInsight(): Promise<void> {
    if (!isTauri()) return;
    await invoke('copilot_cancel_insight');
  }
}

export const copilotService = new CopilotService();

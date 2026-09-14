/**
 * Live Copilot wire types (BACKLOG I1, ADR-0038).
 *
 * These mirror the Rust shapes in `src-tauri/src/copilot/` exactly — the Rust
 * side serialises camelCase, so field names here are the contract. Change one
 * side and you must change the other.
 */

/**
 * What the operating system will actually do when asked to keep the panel out
 * of a screen capture.
 *
 * Three states rather than a boolean on purpose: `bestEffort` is the honest
 * answer on macOS 15+, where ScreenCaptureKit — which Zoom, Meet and Teams use
 * — ignores the request entirely. Rendering it as "protected" would promise
 * invisibility the platform does not deliver; rendering it as "unsupported"
 * would hide that it still blocks legacy capture and screenshot tools.
 */
export type CaptureProtectionLevel = 'enforced' | 'bestEffort' | 'unsupported';

/** The posture plus the wording to show for it. The backend owns both. */
export interface ProtectionVerdict {
  level: CaptureProtectionLevel;
  /** Short chip text for the panel header. */
  headline: string;
  /** The full sentence, for Settings. */
  detail: string;
}

/** One shortcut per action. Canonicalised by the backend on save. */
export interface Keybinds {
  togglePanel: string;
  ask: string;
  captureScreen: string;
}

export interface CopilotConfig {
  /**
   * The live-copilot switch. Default OFF: with it off the backend registers no
   * global shortcut and creates no window. It may default on only after the I9
   * gate (A5 GO + the live smoke).
   */
  enabled: boolean;
  /** Ask the OS to keep the panel out of screen captures. */
  contentProtection: boolean;
  /** Master switch for every global shortcut, separate from `enabled`. */
  shortcutsEnabled: boolean;
  keybinds: Keybinds;
  /**
   * Seconds of recent speech an insight will be built from (I2).
   *
   * **Not a retention setting.** It selects a view over the session buffer and
   * evicts nothing: while a meeting is being recorded the copilot holds all of
   * it in memory whatever this is set to (see `docs/SECURITY_PRIVACY.md`).
   *
   * The backend accepts 30–900. A value outside that range is refused only when
   * the caller changed it, and repaired otherwise — no UI exposes this field,
   * and refusing a stored value would block every other copilot setting,
   * including switching the copilot off.
   */
  liveWindowSecs: number;
  /**
   * May a live insight leave the device (I3a)?
   *
   * **Off by default, and deliberately separate from `enabled`.** A live
   * insight fires from a global hotkey mid-sentence, so sending the last
   * minutes of a conversation to a third-party provider must not follow from a
   * keypress the user set up for something else. With this off the backend
   * refuses any provider it cannot show runs locally and the built-in model
   * answers instead (`docs/DECISIONS.md` ADR-0040).
   *
   * No UI exposes it yet — I3b adds that — but it must survive a round-trip:
   * the backend defaults a missing field to `false`, so dropping it here would
   * silently switch the setting off whenever any other copilot setting is
   * saved.
   */
  allowCloudInsights: boolean;
}

/**
 * The I2 live-context service, as counts. Deliberately no text: the status
 * read is polled by Settings and the panel, and neither needs the transcript
 * from it — the panel already has the `transcript-update` stream.
 */
export interface LiveContextStatus {
  /** Is the service listening to `transcript-update` right now? */
  subscribed: boolean;
  windowSecs: number;
  /** Segments held for this recording session. */
  turns: number;
  /** Segments inside the rolling window. */
  windowTurns: number;
  /** Segments dropped from the front of the session buffer by its size cap. */
  evicted: number;
}

/**
 * A row in the Settings shortcut list. `registered` is the truth about the OS,
 * not about the setting: an action with no implementation yet (`ask`,
 * `captureScreen` — I3 and I6) is never registered, and says why.
 */
export interface ShortcutInfo {
  action: string;
  label: string;
  keybind: string;
  registered: boolean;
  unavailableReason: string | null;
}

export interface CopilotStatus {
  config: CopilotConfig;
  protection: ProtectionVerdict;
  panelOpen: boolean;
  /**
   * Is a recording session running? The panel shows live content only while
   * this is true — the copilot has no capture of its own; it follows a session
   * the user already consented to (ADR-0038 invariant 2).
   */
  recording: boolean;
  shortcuts: ShortcutInfo[];
  liveContext: LiveContextStatus;
  /**
   * The actions the active mode offers (I3b). The panel renders exactly these
   * and no more — which actions exist is the mode's decision, made in Rust.
   */
  liveActions: LiveAction[];
}

/* --- Live insights (BACKLOG I3a/I3b, ADR-0038/0040) ----------------------- */

/**
 * The four things the copilot can be asked for during a conversation.
 *
 * Mirrors `modes::LiveAction`. Which of them a given mode offers is the mode's
 * decision, enforced in Rust — the panel only renders what it is told.
 */
export type LiveAction = 'suggest' | 'followUpQuestions' | 'recap' | 'define';

/** Why a claim the model produced was not shown. Mirrors `ask::grounding::DropReason`. */
export type DropReason = 'ungroundedCitation' | 'blankText' | 'duplicate';

/**
 * A claim that survived grounding.
 *
 * `timestamp` and `audioStartTime` come from the retrieved segment, never from
 * the model — that is the whole point of `ask::grounding`, and it is why a
 * citation here can be trusted to point at something that was actually said.
 */
export interface GroundedClaim {
  text: string;
  sourceChunkId: string;
  timestamp: string;
  audioStartTime: number | null;
}

/**
 * A claim that did not survive.
 *
 * Surfaced rather than hidden: a silently shortened answer looks complete, and
 * the user cannot tell that filtering happened.
 */
export interface DroppedClaim {
  text: string;
  sourceChunkId: string;
  reason: DropReason;
}

/**
 * What an insight request came back as. Mirrors `copilot::insight::LiveInsightOutcome`.
 *
 * **There is deliberately no "answered with nothing" variant.** An empty answer
 * reads as a statement about the conversation ("there is nothing there"), which
 * is a different and unsupported claim — so an empty window and a model that
 * declined both arrive as `noContext` and are rendered as a refusal.
 */
export type LiveInsightOutcome =
  | { status: 'noContext' }
  | {
      status: 'answered';
      action: LiveAction;
      claims: GroundedClaim[];
      dropped: DroppedClaim[];
      turnsConsidered: number;
      turnsOmitted: number;
    }
  | {
      status: 'refused';
      action: LiveAction;
      dropped: DroppedClaim[];
      turnsConsidered: number;
      turnsOmitted: number;
    };

/**
 * Why an insight could not be produced. Mirrors `copilot::commands::InsightFailure`.
 *
 * `kind` is the stable token to branch on; `message` is the backend's own
 * sentence, so the panel never has to invent wording for a refusal it does not
 * own the rules for.
 */
export interface InsightFailure {
  kind:
    | 'actionNotAllowed'
    | 'noUsableSource'
    | 'tenantMismatch'
    | 'cloudNotAllowed'
    | 'provider'
    | 'unparsable'
    | 'timeout'
    | 'cancelled';
  message: string;
}

/** Type guard for the value a rejected `requestInsight` throws. */
export function isInsightFailure(e: unknown): e is InsightFailure {
  return (
    typeof e === 'object' &&
    e !== null &&
    typeof (e as InsightFailure).kind === 'string' &&
    typeof (e as InsightFailure).message === 'string'
  );
}

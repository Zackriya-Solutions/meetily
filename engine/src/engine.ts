// Orquestador del Motor: Segmentos → Turnos → Compuerta → Redacción → Sugerencia (SPEC §4).
// Sin E/S: los proveedores se inyectan, así el pipeline completo se prueba con respuestas grabadas.
import { evaluateGate, slideWindow, type ChoiceEvaluator, type GateDecision, type WindowSegment } from "./gate";
import { buildDraftPrompt, validateDraft, type Drafter } from "./draft";
import type { InboundMessage, SuggestionMessage } from "./protocol";
import { selectBoard, type Role } from "./roles";
import { TurnAssembler, type TurnEvent, type TurnOptions } from "./turns";

export interface EngineConfig {
  window_seconds: number;
  turns: TurnOptions;
  antinoise: {
    /** Mínimo de segundos de audio entre Sugerencias del mismo Rol. */
    min_seconds_between_same_role: number;
    max_suggestions_per_meeting: number;
  };
}

export type EngineLog =
  | { event: "gate_decision"; trigger: TurnEvent["kind"]; turn: number; decision: GateDecision; latency_ms: number }
  | { event: "suggestion_suppressed"; role: string; reason: "cooldown" | "repeated" | "meeting_cap" | "invalid_draft" }
  | { event: "provider_failed"; stage: "gate" | "draft"; error: string };

export interface EngineDeps {
  roles: Role[];
  evaluate: ChoiceEvaluator;
  draft: Drafter;
  config: EngineConfig;
  log?: (entry: EngineLog) => void;
  now?: () => number;
}

export class Engine {
  private board: Role[] = [];
  private segments: WindowSegment[] = [];
  private turns: TurnAssembler;
  private lastByRole = new Map<string, number>();
  private shown = new Set<string>();
  private suggestionCount = 0;

  constructor(private readonly deps: EngineDeps) {
    this.turns = new TurnAssembler(deps.config.turns);
    this.board = selectBoard(deps.roles, undefined);
  }

  /** Procesa un mensaje de la App y devuelve las Sugerencias que hay que mostrar. */
  async handle(message: InboundMessage): Promise<SuggestionMessage[]> {
    switch (message.type) {
      case "session":
        if (message.event === "start") {
          this.reset(message.roles);
          return [];
        }
        return this.process(this.turns.flush());
      case "segment":
        this.segments.push(message);
        return this.process(this.turns.push(message));
      case "clock":
        return this.process(this.turns.tick(message.t));
    }
  }

  private reset(roles: string[] | undefined) {
    this.board = selectBoard(this.deps.roles, roles);
    this.segments = [];
    this.turns = new TurnAssembler(this.deps.config.turns);
    this.lastByRole.clear();
    this.shown.clear();
    this.suggestionCount = 0;
  }

  private async process(events: TurnEvent[]): Promise<SuggestionMessage[]> {
    const out: SuggestionMessage[] = [];
    for (const event of events) {
      const suggestion = await this.evaluate(event);
      if (suggestion) out.push(suggestion);
    }
    return out;
  }

  private async evaluate(event: TurnEvent): Promise<SuggestionMessage | null> {
    const window = slideWindow(
      this.segments.filter((s) => s.t1 <= event.turn.t1),
      this.deps.config.window_seconds,
    );
    const now = this.deps.now ?? (() => performance.now());

    let decision: GateDecision;
    const started = now();
    try {
      decision = await evaluateGate(window, this.board, this.deps.evaluate);
    } catch (error) {
      this.deps.log?.({ event: "provider_failed", stage: "gate", error: String(error) });
      return null;
    }
    this.deps.log?.({
      event: "gate_decision",
      trigger: event.kind,
      turn: event.turn.index,
      decision,
      latency_ms: Math.round(now() - started),
    });
    if (!decision.speak) return null;

    const role = this.board.find((r) => r.id === decision.role)!;
    const { antinoise } = this.deps.config;
    if (this.suggestionCount >= antinoise.max_suggestions_per_meeting) {
      return this.suppress(role.id, "meeting_cap");
    }
    const last = this.lastByRole.get(role.id);
    if (last !== undefined && event.turn.t1 - last < antinoise.min_seconds_between_same_role) {
      return this.suppress(role.id, "cooldown");
    }

    let draft;
    try {
      draft = validateDraft(await this.deps.draft(buildDraftPrompt(role, window)));
    } catch (error) {
      this.deps.log?.({ event: "provider_failed", stage: "draft", error: String(error) });
      return null;
    }
    if (!draft) return this.suppress(role.id, "invalid_draft");

    const key = draft.text.toLowerCase();
    if (this.shown.has(key)) return this.suppress(role.id, "repeated");

    this.shown.add(key);
    this.lastByRole.set(role.id, event.turn.t1);
    this.suggestionCount++;
    return {
      type: "suggestion",
      role: role.id,
      persona: role.persona,
      text: draft.text,
      reason: draft.reason,
      confidence: decision.probability,
    };
  }

  private suppress(role: string, reason: "cooldown" | "repeated" | "meeting_cap" | "invalid_draft"): null {
    this.deps.log?.({ event: "suggestion_suppressed", role, reason });
    return null;
  }
}

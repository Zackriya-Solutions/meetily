// Evals: fixtures etiquetados + respuestas grabadas de la Compuerta (SPEC §8).
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { Engine, type EngineConfig, type EngineLog } from "./engine";
import type { ChoiceAnswer, ChoiceEvaluator, ChoiceQuestion, GateDecision } from "./gate";
import { Speaker } from "./protocol";
import type { Role } from "./roles";

export const Label = z.object({
  turn: z.number().int().nonnegative(),
  speaker: Speaker,
  text: z.string(),
  should_intervene: z.boolean(),
  expected_role: z.string().nullable(),
  reason: z.string(),
});
export type Label = z.infer<typeof Label>;

export const Fixture = z.object({
  id: z.string().min(1),
  description: z.string(),
  roles: z.array(z.string()),
  segments: z.array(z.object({ speaker: Speaker, text: z.string(), t0: z.number(), t1: z.number() })),
  labels: z.array(Label),
});
export type Fixture = z.infer<typeof Fixture>;

export function loadFixtures(dir: string): Fixture[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => Fixture.parse(JSON.parse(readFileSync(join(dir, f), "utf8"))));
}

// Grabaciones

export function recordingKey(model: string, state: string, question: ChoiceQuestion): string {
  return new Bun.CryptoHasher("sha256").update(JSON.stringify({ model, state, question })).digest("hex");
}

export class Recordings {
  private entries: Record<string, ChoiceAnswer>;
  private dirty = false;

  constructor(private readonly path: string) {
    this.entries = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {};
  }

  get(key: string): ChoiceAnswer | undefined {
    return this.entries[key];
  }

  set(key: string, answer: ChoiceAnswer) {
    this.entries[key] = answer;
    this.dirty = true;
  }

  save() {
    if (!this.dirty) return;
    const sorted = Object.fromEntries(Object.entries(this.entries).sort(([a], [b]) => a.localeCompare(b)));
    writeFileSync(this.path, JSON.stringify(sorted, null, 2) + "\n");
    this.dirty = false;
  }
}

/** Reproduce respuestas grabadas. Una pregunta nueva falla: hay que volver a grabar. */
export function replayEvaluator(model: string, recordings: Recordings): ChoiceEvaluator {
  return async (state, question) => {
    const answer = recordings.get(recordingKey(model, state, question));
    if (!answer) throw new Error("missing recording: corre evals/runner/run.ts --record");
    return answer;
  };
}

/** Llama al modelo real y guarda cada respuesta. */
export function recordingEvaluator(model: string, live: ChoiceEvaluator, recordings: Recordings): ChoiceEvaluator {
  return async (state, question) => {
    const answer = await live(state, question);
    recordings.set(recordingKey(model, state, question), answer);
    return answer;
  };
}

// Ejecución

export interface TurnResult {
  label: Label;
  decision: GateDecision;
  latency_ms: number;
}

export interface FixtureRun {
  fixture: Fixture;
  turns: TurnResult[];
  suggestions: number;
  errors: string[];
}

/** Corre una Reunión sintética por el Motor completo y junta la decisión de la Compuerta de cada Turno. */
export async function runFixture(
  fixture: Fixture,
  deps: { roles: Role[]; evaluate: ChoiceEvaluator; config: EngineConfig; now?: () => number },
): Promise<FixtureRun> {
  const logs: EngineLog[] = [];
  const engine = new Engine({
    roles: deps.roles,
    evaluate: deps.evaluate,
    draft: async () => ({ text: `Sugerencia ${logs.length}.`, reason: "Redacción de prueba." }),
    config: deps.config,
    log: (l) => logs.push(l),
    now: deps.now,
  });

  let suggestions = 0;
  await engine.handle({ type: "session", event: "start", roles: fixture.roles });
  for (const s of fixture.segments) suggestions += (await engine.handle({ type: "segment", ...s })).length;
  suggestions += (await engine.handle({ type: "session", event: "end" })).length;

  const decisions = new Map<number, { decision: GateDecision; latency_ms: number }>();
  const errors: string[] = [];
  for (const l of logs) {
    if (l.event === "gate_decision" && l.trigger === "turn_closed") decisions.set(l.turn, l);
    if (l.event === "provider_failed") errors.push(l.error);
  }

  const turns = fixture.labels.map((label) => {
    const d = decisions.get(label.turn);
    if (!d) throw new Error(`${fixture.id}: no hubo decisión de la Compuerta para el Turno ${label.turn}`);
    return { label, decision: d.decision, latency_ms: d.latency_ms };
  });
  return { fixture, turns, suggestions, errors };
}

export interface Metrics {
  turns: number;
  precision: number;
  recall: number;
  role_accuracy: number;
  latency_p50_ms: number;
  latency_p90_ms: number;
}

export function computeMetrics(runs: FixtureRun[]): Metrics {
  const all = runs.flatMap((r) => r.turns);
  const tp = all.filter((t) => t.decision.speak && t.label.should_intervene).length;
  const fp = all.filter((t) => t.decision.speak && !t.label.should_intervene).length;
  const fn = all.filter((t) => !t.decision.speak && t.label.should_intervene).length;
  const hits = all.filter((t) => t.decision.speak && t.label.should_intervene);
  const roleHits = hits.filter((t) => t.decision.speak && t.decision.role === t.label.expected_role).length;
  const lat = all.map((t) => t.latency_ms).sort((a, b) => a - b);
  const pct = (p: number) => (lat.length ? lat[Math.min(lat.length - 1, Math.round(p * (lat.length - 1)))]! : 0);
  const ratio = (a: number, b: number) => (b === 0 ? 0 : Math.round((a / b) * 100) / 100);
  return {
    turns: all.length,
    precision: ratio(tp, tp + fp),
    recall: ratio(tp, tp + fn),
    role_accuracy: ratio(roleHits, hits.length),
    latency_p50_ms: pct(0.5),
    latency_p90_ms: pct(0.9),
  };
}

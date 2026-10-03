import { describe, expect, test } from "bun:test";
import { Engine, type EngineConfig, type EngineLog } from "./engine";
import type { ChoiceAnswer, ChoiceEvaluator } from "./gate";
import type { Drafter } from "./draft";
import type { Role } from "./roles";

const config: EngineConfig = {
  window_seconds: 90,
  turns: { gapSeconds: 0.7, continuousSeconds: 10 },
  antinoise: { min_seconds_between_same_role: 30, max_suggestions_per_meeting: 20 },
};

const role = (id: string, gate_option: string, persona: string, threshold: number): Role => ({
  id,
  role: id.toUpperCase(),
  persona,
  objective: "o",
  gate_option,
  gate_definition: "d",
  limits: [],
  sources: [],
  threshold,
  calibrated_with: "none",
  status: "active",
  instructions: "i",
});

const roles = [role("cfo", "cfo", "Betty", 0.75), role("ceo", "ceo_adversarial", "Sheldon", 0.85)];

/** Elige cfo cuando la ventana menciona plata; si no, none. */
const moneyGate: ChoiceEvaluator = async (state): Promise<ChoiceAnswer> =>
  /millones|IVA|precio/i.test(state)
    ? { choice: "cfo", probabilities: { cfo: 0.9 } }
    : { choice: "none", probabilities: { none: 0.95 } };

let drafts = 0;
const drafter: Drafter = async () => ({ text: `Pregunta si incluye IVA (${++drafts}).`, reason: "Precio sin impuestos." });

const seg = (speaker: "user" | "counterpart", t0: number, t1: number, text: string) =>
  ({ type: "segment", speaker, text, t0, t1 }) as const;

function build(overrides: Partial<{ evaluate: ChoiceEvaluator; draft: Drafter; config: EngineConfig }> = {}) {
  const logs: EngineLog[] = [];
  const engine = new Engine({
    roles,
    evaluate: overrides.evaluate ?? moneyGate,
    draft: overrides.draft ?? drafter,
    config: overrides.config ?? config,
    log: (l) => logs.push(l),
    now: () => 0,
  });
  return { engine, logs };
}

describe("Engine", () => {
  test("emite la Sugerencia de Betty al cerrar el Turno de la Contraparte", async () => {
    const { engine } = build();
    await engine.handle({ type: "session", event: "start", roles: ["cfo", "ceo"] });
    expect(await engine.handle(seg("counterpart", 0, 3, "El plan anual cuesta dos millones."))).toEqual([]);
    const out = await engine.handle(seg("user", 3.1, 4, "Ok."));
    expect(out).toEqual([
      expect.objectContaining({ type: "suggestion", role: "cfo", persona: "Betty", confidence: 0.9 }),
    ]);
  });

  test("no habla cuando la Compuerta elige none", async () => {
    const { engine, logs } = build();
    await engine.handle(seg("counterpart", 0, 3, "¿Cómo estuvo el fin de semana?"));
    expect(await engine.handle({ type: "session", event: "end" })).toEqual([]);
    expect(logs.find((l) => l.event === "gate_decision")).toMatchObject({ decision: { speak: false } });
  });

  test("respeta 30 s entre Sugerencias del mismo Rol", async () => {
    const { engine, logs } = build();
    await engine.handle(seg("counterpart", 0, 3, "Son dos millones."));
    expect(await engine.handle(seg("user", 3.1, 4, "Ok."))).toHaveLength(1);
    await engine.handle(seg("counterpart", 4.1, 8, "Más IVA."));
    expect(await engine.handle(seg("user", 8.1, 9, "Ok."))).toEqual([]);
    expect(logs).toContainEqual({ event: "suggestion_suppressed", role: "cfo", reason: "cooldown" });
    await engine.handle(seg("counterpart", 40, 44, "El precio sube en marzo."));
    expect(await engine.handle(seg("user", 44.1, 45, "Ok."))).toHaveLength(1);
  });

  test("no repite una Sugerencia ya mostrada", async () => {
    const { engine, logs } = build({ draft: async () => ({ text: "Pide el desglose.", reason: "Total sin detalle." }) });
    await engine.handle(seg("counterpart", 0, 3, "Son dos millones."));
    await engine.handle(seg("user", 3.1, 4, "Ok."));
    await engine.handle(seg("counterpart", 50, 53, "El precio final es otro."));
    expect(await engine.handle(seg("user", 53.1, 54, "Ok."))).toEqual([]);
    expect(logs).toContainEqual({ event: "suggestion_suppressed", role: "cfo", reason: "repeated" });
  });

  test("aplica el tope de Sugerencias por Reunión", async () => {
    const { engine } = build({ config: { ...config, antinoise: { min_seconds_between_same_role: 0, max_suggestions_per_meeting: 1 } } });
    await engine.handle(seg("counterpart", 0, 3, "Son dos millones."));
    expect(await engine.handle(seg("user", 3.1, 4, "Ok."))).toHaveLength(1);
    await engine.handle(seg("counterpart", 5, 8, "Más IVA."));
    expect(await engine.handle(seg("user", 8.1, 9, "Ok."))).toEqual([]);
  });

  test("descarta una Redacción de más de 15 palabras", async () => {
    const { engine, logs } = build({
      draft: async () => ({ text: "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciséis", reason: "x" }),
    });
    await engine.handle(seg("counterpart", 0, 3, "Son dos millones."));
    expect(await engine.handle(seg("user", 3.1, 4, "Ok."))).toEqual([]);
    expect(logs).toContainEqual({ event: "suggestion_suppressed", role: "cfo", reason: "invalid_draft" });
  });

  test("si falla el proveedor de la Compuerta, no se cae y registra el error", async () => {
    const { engine, logs } = build({ evaluate: async () => { throw new Error("timeout"); } });
    await engine.handle(seg("counterpart", 0, 3, "Son dos millones."));
    expect(await engine.handle(seg("user", 3.1, 4, "Ok."))).toEqual([]);
    expect(logs).toContainEqual({ event: "provider_failed", stage: "gate", error: "Error: timeout" });
  });

  test("la ventana de la Compuerta no incluye Segmentos posteriores al Turno evaluado", async () => {
    const seen: string[] = [];
    const { engine } = build({ evaluate: async (state) => { seen.push(state); return { choice: "none", probabilities: { none: 1 } }; } });
    await engine.handle(seg("counterpart", 0, 3, "Primero."));
    await engine.handle(seg("user", 3.1, 4, "Después."));
    expect(seen[0]).toContain("Primero.");
    expect(seen[0]).not.toContain("Después.");
  });

  test("la sesión solo activa Roles pedidos y activos", async () => {
    const seen: string[] = [];
    const { engine } = build({
      evaluate: async (_s, q) => { seen.push(Object.keys(q.criteria).join(",")); return { choice: "none", probabilities: { none: 1 } }; },
    });
    await engine.handle({ type: "session", event: "start", roles: ["cfo"] });
    await engine.handle(seg("counterpart", 0, 3, "x"));
    await engine.handle({ type: "session", event: "end" });
    expect(seen).toEqual(["none,cfo"]);
  });
});

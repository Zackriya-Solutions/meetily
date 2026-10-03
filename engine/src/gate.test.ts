import { describe, expect, test } from "bun:test";
import {
  buildGateQuestion,
  evaluateGate,
  slideWindow,
  type ChoiceEvaluator,
  type GateRole,
  type WindowSegment,
} from "./gate";

const cfo: GateRole = {
  id: "cfo",
  gate_option: "cfo",
  gate_definition: "Interviene cuando se mencionan cifras, precios, impuestos o condiciones de pago sin aclarar.",
  threshold: 0.75,
};
const ceo: GateRole = {
  id: "ceo",
  gate_option: "ceo_adversarial",
  gate_definition: "Interviene cuando se acepta un acuerdo sin datos.",
  threshold: 0.85,
};

const price: WindowSegment[] = [
  { speaker: "counterpart", text: "El plan anual cuesta dos millones.", t0: 10, t1: 12 },
];

const answering =
  (choice: string, p: number): ChoiceEvaluator =>
  async () => ({ choice, probabilities: { [choice]: p } });

describe("evaluateGate", () => {
  test("habla cuando la opción elegida supera el umbral del Rol", async () => {
    expect(await evaluateGate(price, [cfo, ceo], answering("cfo", 0.9))).toEqual({
      speak: true,
      role: "cfo",
      probability: 0.9,
    });
  });

  test("calla bajo el umbral del Rol", async () => {
    const d = await evaluateGate(price, [cfo, ceo], answering("ceo_adversarial", 0.8));
    expect(d.speak).toBe(false);
  });

  test("traduce gate_option a id del Rol", async () => {
    const d = await evaluateGate(price, [cfo, ceo], answering("ceo_adversarial", 0.95));
    expect(d).toMatchObject({ speak: true, role: "ceo" });
  });

  test("calla cuando Jev elige none", async () => {
    expect((await evaluateGate(price, [cfo], answering("none", 0.99))).speak).toBe(false);
  });

  test("calla ante una opción desconocida", async () => {
    expect((await evaluateGate(price, [cfo], answering("betty", 0.99))).speak).toBe(false);
  });

  test("no llama al proveedor sin Segmentos", async () => {
    let called = false;
    await evaluateGate([], [cfo], async () => {
      called = true;
      return { choice: "cfo" };
    });
    expect(called).toBe(false);
  });

  test("la Persona nunca llega al proveedor", async () => {
    let seen = "";
    const spy: ChoiceEvaluator = async (state, q) => {
      seen = state + JSON.stringify(q);
      return { choice: "none", probabilities: { none: 1 } };
    };
    await evaluateGate(price, [{ ...cfo, persona: "Betty" } as GateRole], spy);
    expect(seen).not.toContain("Betty");
  });
});

describe("buildGateQuestion", () => {
  test("incluye none y una opción por Rol", () => {
    expect(Object.keys(buildGateQuestion([cfo, ceo]).criteria)).toEqual(["none", "cfo", "ceo_adversarial"]);
  });

  test("rechaza gate_option duplicado o reservado", () => {
    expect(() => buildGateQuestion([cfo, { ...ceo, gate_option: "cfo" }])).toThrow();
    expect(() => buildGateQuestion([{ ...cfo, gate_option: "none" }])).toThrow();
  });
});

describe("slideWindow", () => {
  test("conserva solo los últimos N segundos", () => {
    const segs: WindowSegment[] = [
      { speaker: "user", text: "a", t0: 0, t1: 5 },
      { speaker: "counterpart", text: "b", t0: 50, t1: 60 },
      { speaker: "counterpart", text: "c", t0: 100, t1: 110 },
    ];
    expect(slideWindow(segs, 90).map((s) => s.text)).toEqual(["b", "c"]);
  });
});

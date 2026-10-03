import { describe, expect, test } from "bun:test";
import { buildDraftPrompt, countWords, validateDraft } from "./draft";

const betty = {
  role: "CFO",
  persona: "Betty",
  objective: "Proteger la caja.",
  limits: ["legal opinions"],
  instructions: "Cuidas la caja.",
};

describe("buildDraftPrompt", () => {
  test("la Persona y las instrucciones del Rol van en el system", () => {
    const p = buildDraftPrompt(betty, [{ speaker: "counterpart", text: "Son dos millones.", t0: 0, t1: 2 }]);
    expect(p.system).toContain("Betty");
    expect(p.system).toContain("Cuidas la caja.");
    expect(p.system).toContain("15 palabras");
    expect(p.prompt).toContain("[Contraparte] Son dos millones.");
  });
});

describe("validateDraft", () => {
  test("acepta una Sugerencia de hasta 15 palabras", () => {
    expect(validateDraft({ text: " Pregunta si ese valor incluye IVA. ", reason: "Precio sin impuestos." })).toEqual({
      text: "Pregunta si ese valor incluye IVA.",
      reason: "Precio sin impuestos.",
    });
  });

  test("rechaza más de 15 palabras en vez de recortarlas", () => {
    const long = "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciséis";
    expect(countWords(long)).toBe(16);
    expect(validateDraft({ text: long, reason: "x" })).toBeNull();
  });

  test("deja el motivo en una sola línea", () => {
    expect(validateDraft({ text: "Pide el desglose.", reason: "Mencionó un total\nsin detalle." })?.reason).toBe(
      "Mencionó un total sin detalle.",
    );
  });

  test("rechaza texto o motivo vacíos", () => {
    expect(validateDraft({ text: "  ", reason: "x" })).toBeNull();
    expect(validateDraft({ text: "Pide el desglose.", reason: " " })).toBeNull();
  });
});

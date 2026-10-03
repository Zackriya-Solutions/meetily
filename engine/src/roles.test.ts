import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { loadRoles, parseRole, selectBoard } from "./roles";

const ROLES_DIR = join(import.meta.dir, "../../roles");

const valid = `---
role: CFO
persona: Betty
objective: Proteger la caja.
gate_option: cfo
gate_definition: Interviene con cifras sin aclarar.
limits: [legal opinions]
sources: [finance]
threshold: 0.75
calibrated_with: none
status: active
---
Instrucciones.`;

describe("roles/ del repo", () => {
  const roles = loadRoles(ROLES_DIR);

  test("todos los archivos cumplen el esquema", () => {
    expect(roles.map((r) => r.id)).toEqual(["ceo", "cfo", "cmo", "cto"]);
  });

  test("la Junta del MVP es CFO + CEO adversarial", () => {
    expect(selectBoard(roles, undefined).map((r) => r.id)).toEqual(["ceo", "cfo"]);
  });

  test("CTO y CMO son experimentales y no entran a la Junta aunque se pidan", () => {
    expect(selectBoard(roles, ["cfo", "cto", "cmo"]).map((r) => r.id)).toEqual(["cfo"]);
  });

  test("el umbral de Sheldon es más alto que el de Betty", () => {
    const byId = Object.fromEntries(roles.map((r) => [r.id, r]));
    expect(byId.ceo!.threshold).toBeGreaterThan(byId.cfo!.threshold);
  });
});

describe("parseRole", () => {
  test("lee frontmatter e instrucciones", () => {
    const role = parseRole("cfo", valid);
    expect(role).toMatchObject({ id: "cfo", persona: "Betty", gate_option: "cfo", instructions: "Instrucciones." });
  });

  test("rechaza claves en español", () => {
    expect(() => parseRole("cfo", valid.replace("threshold: 0.75", "umbral: 0.75"))).toThrow(/threshold/);
  });

  test("rechaza un status inválido", () => {
    expect(() => parseRole("cfo", valid.replace("status: active", "status: on"))).toThrow(/status/);
  });

  test("rechaza ids que no son snake_case", () => {
    expect(() => parseRole("CFO-Role", valid)).toThrow(/snake_case/);
  });

  test("rechaza un Rol sin instrucciones", () => {
    expect(() => parseRole("cfo", valid.replace("Instrucciones.", ""))).toThrow(/instrucciones/);
  });
});

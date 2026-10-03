// Contrato + E2E del Motor: las Reuniones sintéticas de evals/ corren por el pipeline completo
// con las respuestas grabadas de Jev (sin keys, determinista). Si cambia el prompt de la Compuerta
// o un Rol, falta la grabación y la prueba falla: hay que volver a grabar (evals/FORMAT.md).
import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import config from "../config.json";
import { loadFixtures, Recordings, replayEvaluator, runFixture } from "./evals";
import { loadRoles } from "./roles";

const ROOT = join(import.meta.dir, "../..");
const fixtures = loadFixtures(join(ROOT, "evals/fixtures"));
const recordings = new Recordings(join(ROOT, `evals/recordings/${config.gate.model}.json`));
const roles = loadRoles(join(ROOT, "roles"));

describe("contrato de la Compuerta con respuestas grabadas", () => {
  test("hay al menos 5 Reuniones sintéticas", () => {
    expect(fixtures.length).toBeGreaterThanOrEqual(5);
  });

  for (const fixture of fixtures) {
    test(fixture.id, async () => {
      const run = await runFixture(fixture, {
        roles,
        evaluate: replayEvaluator(config.gate.model, recordings),
        config,
        now: () => 0,
      });
      expect(run.errors).toEqual([]);
      const decisions = run.turns.map((t) => (t.decision.speak ? t.decision.role : "none"));
      expect({ decisions, suggestions: run.suggestions }).toMatchSnapshot();
    });
  }

  test("las etiquetas solo usan ids de Rol activos", () => {
    const ids = new Set(roles.filter((r) => r.status === "active").map((r) => r.id));
    for (const f of fixtures) {
      for (const l of f.labels) {
        if (l.should_intervene) expect(ids.has(l.expected_role!)).toBe(true);
        else expect(l.expected_role).toBeNull();
      }
    }
  });
});

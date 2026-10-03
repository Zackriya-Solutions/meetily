// Runner de evals de la Compuerta. Informe, no bloquea PRs (SPEC §9).
//   bun evals/runner/run.ts            reproduce las respuestas grabadas
//   bun evals/runner/run.ts --record   llama a Jev (TYPESAFE_AI_API_KEY) y graba
// SOTTOLY_EVALS_DIR=<ruta a sottoly-evals> agrega las Reuniones reales del dataset privado.
import { join } from "node:path";
import config from "../../engine/config.json";
import {
  computeMetrics,
  loadFixtures,
  recordingEvaluator,
  Recordings,
  replayEvaluator,
  runFixture,
} from "../../engine/src/evals";
import { jevEvaluator } from "../../engine/src/providers";
import { loadRoles } from "../../engine/src/roles";

const ROOT = join(import.meta.dir, "../..");
const record = process.argv.includes("--record");
const model = config.gate.model;

const fixtures = [
  ...loadFixtures(join(ROOT, "evals/fixtures")),
  ...(process.env.SOTTOLY_EVALS_DIR ? loadFixtures(join(process.env.SOTTOLY_EVALS_DIR, "fixtures")) : []),
];
const recordings = new Recordings(join(ROOT, `evals/recordings/${model}.json`));
const evaluate = record ? recordingEvaluator(model, jevEvaluator(model), recordings) : replayEvaluator(model, recordings);
const roles = loadRoles(join(ROOT, "roles"));

const runs = [];
for (const fixture of fixtures) {
  const run = await runFixture(fixture, { roles, evaluate, config, now: record ? undefined : () => 0 });
  runs.push(run);
  for (const t of run.turns) {
    const got = t.decision.speak ? t.decision.role : "none";
    const want = t.label.should_intervene ? t.label.expected_role : "none";
    const mark = got === want ? "✓" : "✗";
    console.log(`${mark} ${fixture.id}#${t.label.turn} [${t.label.speaker}] esperado=${want} obtenido=${got} p=${t.decision.probability} — ${t.label.text}`);
  }
  for (const e of run.errors) console.log(`! ${fixture.id}: ${e}`);
}
recordings.save();

const metrics = computeMetrics(runs);
console.log("\n" + JSON.stringify({ model, mode: record ? "record" : "replay", fixtures: fixtures.length, ...metrics }, null, 2));
if (!record) console.log("(replay: la latencia no es real; usa --record para medirla)");

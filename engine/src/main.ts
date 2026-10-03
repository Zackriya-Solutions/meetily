// Motor (sidecar): lee mensajes por stdin y escribe por stdout, una línea JSON por mensaje.
// Hola Jev: por cada Segmento de la Contraparte evalúa la Compuerta y reporta la decisión por stderr.
// La Redacción todavía no existe, así que stdout no emite Sugerencias aún.
import { experimental_evaluate } from "ai";
import { createTypeSafeAi } from "@ai-sdk/typesafe-ai";
import config from "../config.json";
import { evaluateGate, slideWindow, type ChoiceEvaluator, type GateRole, type WindowSegment } from "./gate";
import { parseInbound } from "./protocol";

// Rol de prueba hasta que exista el cargador de roles/ (Fase 3).
const HELLO_ROLES: GateRole[] = [
  {
    id: "cfo",
    gate_option: "cfo",
    gate_definition: "Interviene cuando se mencionan cifras, precios, impuestos o condiciones de pago sin aclarar.",
    threshold: 0.75,
  },
];

function jevEvaluator(): ChoiceEvaluator {
  const provider = createTypeSafeAi(); // lee TYPESAFE_AI_API_KEY
  const model = provider.evaluationModel(config.gate.model);
  return async (state, question) => {
    const { answers } = await experimental_evaluate({ model, state, questions: { gate: question } });
    const a = answers.gate;
    if (a.type !== "choice") throw new Error(`unexpected answer type ${a.type}`);
    return { choice: a.choice, probabilities: a.probabilities };
  };
}

function log(event: Record<string, unknown>) {
  process.stderr.write(JSON.stringify({ ...event, at: new Date().toISOString() }) + "\n");
}

async function main() {
  const evaluate = jevEvaluator();
  const segments: WindowSegment[] = [];

  for await (const line of console) {
    if (!line.trim()) continue;
    const parsed = parseInbound(line);
    if (!parsed.ok) {
      log({ level: "warn", event: "invalid_message", error: parsed.error });
      continue;
    }
    const msg = parsed.message;
    if (msg.type !== "segment") continue;

    segments.push(msg);
    if (msg.speaker !== "counterpart") continue;

    const started = performance.now();
    try {
      const decision = await evaluateGate(slideWindow(segments, config.window_seconds), HELLO_ROLES, evaluate);
      log({ event: "gate_decision", ...decision, latency_ms: Math.round(performance.now() - started) });
    } catch (error) {
      log({ level: "error", event: "gate_failed", error: String(error) });
    }
  }
}

main();

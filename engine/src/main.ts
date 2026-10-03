// Motor (sidecar): lee mensajes por stdin y escribe Sugerencias por stdout, una línea JSON por mensaje.
// Los registros (decisiones de la Compuerta, errores) van por stderr para no ensuciar el protocolo.
import { join } from "node:path";
import config from "../config.json";
import { Engine } from "./engine";
import { encodeOutbound, parseInbound } from "./protocol";
import { anthropicDrafter, jevEvaluator } from "./providers";
import { loadRoles } from "./roles";

// En el binario compilado no hay carpeta de fuentes: la App pasa la ruta de roles/.
const ROLES_DIR = process.env.SOTTOLY_ROLES_DIR ?? join(import.meta.dir, "../../roles");

function log(entry: Record<string, unknown>) {
  process.stderr.write(JSON.stringify({ ...entry, at: new Date().toISOString() }) + "\n");
}

async function main() {
  const engine = new Engine({
    roles: loadRoles(ROLES_DIR),
    evaluate: jevEvaluator(config.gate.model),
    draft: anthropicDrafter(config.draft.model, config.draft.max_tokens),
    config,
    log,
  });

  for await (const line of console) {
    if (!line.trim()) continue;
    const parsed = parseInbound(line);
    if (!parsed.ok) {
      log({ level: "warn", event: "invalid_message", error: parsed.error });
      continue;
    }
    for (const suggestion of await engine.handle(parsed.message)) {
      process.stdout.write(encodeOutbound(suggestion));
    }
  }
  for (const suggestion of await engine.handle({ type: "session", event: "end" })) {
    process.stdout.write(encodeOutbound(suggestion));
  }
}

main();

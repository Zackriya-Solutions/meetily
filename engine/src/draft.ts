// Redacción: escribe la Sugerencia del Rol elegido por la Compuerta (SPEC §4).
// La Persona solo entra aquí (tono) y en la UI; nunca en la Compuerta.
import { z } from "zod";
import type { WindowSegment } from "./gate";
import { renderWindow } from "./gate";
import type { Role } from "./roles";

export const MAX_SUGGESTION_WORDS = 15;

export const Draft = z.object({
  text: z.string().min(1),
  reason: z.string().min(1),
});
export type Draft = z.infer<typeof Draft>;

export interface DraftPrompt {
  system: string;
  prompt: string;
}

/** Proveedor intercambiable (Sonnet por defecto; Haiku si la latencia lo exige). */
export type Drafter = (prompt: DraftPrompt) => Promise<Draft>;

export function buildDraftPrompt(role: Pick<Role, "role" | "persona" | "objective" | "limits" | "instructions">, window: WindowSegment[]): DraftPrompt {
  const system = [
    `Eres ${role.persona}, ${role.role} de la junta asesora del Usuario.`,
    `Objetivo: ${role.objective}`,
    role.instructions,
    `No opines sobre: ${role.limits.join(", ")}.`,
    `Responde con una sugerencia para el Usuario de máximo ${MAX_SUGGESTION_WORDS} palabras, en el idioma de la reunión, y el motivo en una línea.`,
  ].join("\n\n");
  const prompt = `Transcripción reciente de la reunión:\n${renderWindow(window)}\n\n¿Qué debería hacer o preguntar el Usuario ahora?`;
  return { system, prompt };
}

export function countWords(text: string): number {
  return text.trim().split(/\s+/).filter(Boolean).length;
}

/** Devuelve la Sugerencia limpia, o null si incumple el formato (no se muestra). */
export function validateDraft(draft: Draft): Draft | null {
  const text = draft.text.trim();
  const reason = draft.reason.trim().replace(/\s*\n\s*/g, " ");
  if (!text || !reason) return null;
  if (countWords(text) > MAX_SUGGESTION_WORDS) return null;
  return { text, reason };
}

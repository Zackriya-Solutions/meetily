// Proveedores reales (ModelProvider): Jev para la Compuerta y Anthropic para la Redacción.
// Los IDs de modelo vienen de config.json; las keys, del entorno (TYPESAFE_AI_API_KEY, ANTHROPIC_API_KEY).
import { experimental_evaluate, generateText, Output } from "ai";
import { createAnthropic } from "@ai-sdk/anthropic";
import { createTypeSafeAi } from "@ai-sdk/typesafe-ai";
import { Draft, type Drafter } from "./draft";
import type { ChoiceEvaluator } from "./gate";

export function jevEvaluator(modelId: string): ChoiceEvaluator {
  const model = createTypeSafeAi().evaluationModel(modelId);
  return async (state, question) => {
    const { answers } = await experimental_evaluate({ model, state, questions: { gate: question } });
    const answer = answers.gate;
    if (answer.type !== "choice") throw new Error(`unexpected answer type ${answer.type}`);
    return { choice: answer.choice, probabilities: answer.probabilities };
  };
}

export function anthropicDrafter(modelId: string, maxTokens: number): Drafter {
  const model = createAnthropic()(modelId);
  return async ({ system, prompt }) => {
    const { output } = await generateText({
      model,
      system,
      prompt,
      maxOutputTokens: maxTokens,
      output: Output.object({ schema: Draft }),
    });
    return Draft.parse(output);
  };
}

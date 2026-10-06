import type { MeetingSpeaker } from '@/types';
import type { ModelConfig } from '@/services/configService';

type SummaryModelChoice = Pick<ModelConfig, 'provider' | 'customOpenAIEndpoint'> | null;

const LOCAL_PROVIDERS: ReadonlyArray<ModelConfig['provider']> = ['ollama', 'builtin-ai'];
const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1']);

/** True when the summary model runs on this machine, so the transcript does not leave it. */
export function isLocalSummaryModel(config: SummaryModelChoice): boolean {
  if (!config) return false;
  if (LOCAL_PROVIDERS.includes(config.provider)) return true;
  if (config.provider !== 'custom-openai' || !config.customOpenAIEndpoint) return false;
  try {
    return LOCAL_HOSTS.has(new URL(config.customOpenAIEndpoint).hostname);
  } catch {
    return false;
  }
}

/** A speaker who still talks in the transcript and has no name. */
export function hasUnnamedSpeaker(speakers: MeetingSpeaker[]): boolean {
  return speakers.some((s) => s.row_count > 0 && !s.display_name?.trim());
}

/**
 * Whether to guess names right after Identify. The transcript goes to the summary model, so a
 * cloud model is used only when the user already sends transcripts there by turning on
 * auto-summary; otherwise only the "Guess names" button sends it.
 */
export function shouldAutoGuessNames(input: {
  speakerIdentification: boolean;
  modelConfig: SummaryModelChoice;
  /** The user's saved config was read. Until then, and after a failed load, `modelConfig` is the
   *  Ollama placeholder, not the model the backend will use. */
  modelConfigLoaded: boolean;
  isAutoSummary: boolean;
  speakers: MeetingSpeaker[];
}): boolean {
  const local = input.modelConfigLoaded && isLocalSummaryModel(input.modelConfig);
  return input.speakerIdentification
    && hasUnnamedSpeaker(input.speakers)
    && (local || input.isAutoSummary);
}

export function formatNamingResult(named: number, suggested: number): string {
  if (named > 0 && suggested > 0) return `Named ${named}, suggested ${suggested}`;
  if (named > 0) return `Named ${named}`;
  if (suggested > 0) return `Suggested ${suggested}`;
  return 'No names found';
}

/** The auto-summary waits (up to its cap) for Identify and for the automatic name guess it starts. */
export function isWaitingForSpeakers(s: { expired: boolean; statusKnown: boolean; isActive: boolean; autoNamingPending: boolean }): boolean {
  return !s.expired && (!s.statusKnown || s.isActive || s.autoNamingPending);
}

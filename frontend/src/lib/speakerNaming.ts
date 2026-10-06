import type { MeetingSpeaker } from '@/types';
import type { ModelConfig } from '@/services/configService';

type SummaryModelChoice = Pick<ModelConfig, 'provider'> & Partial<Pick<ModelConfig, 'customOpenAIEndpoint' | 'ollamaEndpoint'>> | null;

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1']);

function isLoopbackUrl(url: string): boolean {
  try {
    return LOCAL_HOSTS.has(new URL(url).hostname);
  } catch {
    return false;
  }
}

/**
 * True when the summary model runs on this machine, so the transcript does not leave it. Ollama
 * and custom endpoints count only when they point at this machine.
 */
export function isLocalSummaryModel(config: SummaryModelChoice): boolean {
  if (!config) return false;
  if (config.provider === 'builtin-ai') return true;
  if (config.provider === 'ollama') {
    // An unset endpoint means the default local server.
    return !config.ollamaEndpoint?.trim() || isLoopbackUrl(config.ollamaEndpoint);
  }
  if (config.provider !== 'custom-openai' || !config.customOpenAIEndpoint) return false;
  return isLoopbackUrl(config.customOpenAIEndpoint);
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

/** The model an automatic guess was approved for; the backend skips the job if it changed. */
export interface AutoGuessApproval {
  /** Null when the saved model could not be read (the user accepted cloud via auto-summary). */
  provider: string | null;
  endpoint: string | null;
}

/**
 * Decides the automatic guess from the model that is saved, which is what the backend names with.
 * The in-memory config can hold an unsaved pick from the model dialog, so it is not consulted;
 * a read that fails counts as a cloud model. Returns the approved model, or null for no guess.
 */
export async function decideAutoGuessNames(input: {
  speakerIdentification: boolean;
  modelConfigLoaded: boolean;
  isAutoSummary: boolean;
  speakers: MeetingSpeaker[];
  readSavedModel: () => Promise<NonNullable<SummaryModelChoice>>;
}): Promise<AutoGuessApproval | null> {
  let saved: SummaryModelChoice = null;
  if (input.modelConfigLoaded) {
    try {
      saved = await input.readSavedModel();
    } catch (error) {
      console.error('Could not read the saved summary model; treating it as a cloud model:', error);
    }
  }
  const approved = shouldAutoGuessNames({
    speakerIdentification: input.speakerIdentification,
    modelConfig: saved,
    modelConfigLoaded: input.modelConfigLoaded,
    isAutoSummary: input.isAutoSummary,
    speakers: input.speakers,
  });
  if (!approved) return null;
  if (!saved) return { provider: null, endpoint: null };
  const endpoint = saved.provider === 'ollama'
    ? saved.ollamaEndpoint
    : saved.provider === 'custom-openai' ? saved.customOpenAIEndpoint : null;
  return { provider: saved.provider, endpoint: endpoint ?? null };
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

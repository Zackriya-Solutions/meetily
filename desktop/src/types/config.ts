export type TranscriptModelProvider =
  | 'localWhisper'
  | 'parakeet'
  | 'deepgram'
  | 'elevenLabs'
  | 'groq'
  | 'openai';

export interface TranscriptModelProps {
  provider: TranscriptModelProvider;
  model: string;
  apiKey?: string | null;
}

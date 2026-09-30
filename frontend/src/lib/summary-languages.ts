import { uiText, languageLabel } from '@/i18n/ui';
export interface LanguageOption {
  code: string;
  label: string;
}

/**
 * Language options offered in the summary language pickers.
 * Codes must stay in sync with `language_name_from_code` in
 * `frontend/src-tauri/src/summary/processor.rs`.
 */
export const LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'en', get label() { return uiText("messages.english"); } },
  { code: 'zh', get label() { return uiText("messages.chinese"); } },
  { code: 'zh-tw', get label() { return uiText("messages.traditionalChinese"); } },
  { code: 'de', get label() { return languageLabel('de', 'German'); } },
  { code: 'es', get label() { return languageLabel('es', 'Spanish'); } },
  { code: 'ru', get label() { return languageLabel('ru', 'Russian'); } },
  { code: 'ko', get label() { return languageLabel('ko', 'Korean'); } },
  { code: 'fr', get label() { return languageLabel('fr', 'French'); } },
  { code: 'ja', get label() { return languageLabel('ja', 'Japanese'); } },
  { code: 'pt', get label() { return languageLabel('pt', 'Portuguese'); } },
  { code: 'it', get label() { return languageLabel('it', 'Italian'); } },
  { code: 'nl', get label() { return languageLabel('nl', 'Dutch'); } },
  { code: 'pl', get label() { return languageLabel('pl', 'Polish'); } },
  { code: 'ar', get label() { return languageLabel('ar', 'Arabic'); } },
  { code: 'hi', get label() { return languageLabel('hi', 'Hindi'); } },
  { code: 'ta', get label() { return languageLabel('ta', 'Tamil'); } },
  { code: 'tr', get label() { return languageLabel('tr', 'Turkish'); } },
  { code: 'vi', get label() { return languageLabel('vi', 'Vietnamese'); } },
  { code: 'th', get label() { return languageLabel('th', 'Thai'); } },
  { code: 'id', get label() { return languageLabel('id', 'Indonesian'); } },
  { code: 'sv', get label() { return languageLabel('sv', 'Swedish'); } },
  { code: 'cs', get label() { return languageLabel('cs', 'Czech'); } },
  { code: 'da', get label() { return languageLabel('da', 'Danish'); } },
  { code: 'fi', get label() { return languageLabel('fi', 'Finnish'); } },
  { code: 'el', get label() { return languageLabel('el', 'Greek'); } },
  { code: 'he', get label() { return languageLabel('he', 'Hebrew'); } },
  { code: 'hu', get label() { return languageLabel('hu', 'Hungarian'); } },
  { code: 'no', get label() { return languageLabel('no', 'Norwegian'); } },
  { code: 'ro', get label() { return languageLabel('ro', 'Romanian'); } },
  { code: 'uk', get label() { return languageLabel('uk', 'Ukrainian'); } },
];

export const AUTO_VALUE = '__auto__' as const;

const SUPPORTED_CODES: ReadonlySet<string> = new Set(LANGUAGE_OPTIONS.map((o) => o.code));

/**
 * Normalises a raw locale string (from transcription or storage) into a code we
 * can translate into. Handles BCP-47 regional tags: `pt-BR` -> `pt`, `en_GB` -> `en`.
 * Returns null for unsupported languages so callers can fall back to English
 * rather than sending a code Rust will silently drop.
 */
export function normaliseLanguageCode(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const lower = raw.toLowerCase().replace(/_/g, '-');
  if (SUPPORTED_CODES.has(lower)) return lower;
  const base = lower.split('-')[0];
  if (SUPPORTED_CODES.has(base)) return base;
  return null;
}

export function labelForCode(code: string): string {
  return LANGUAGE_OPTIONS.find((l) => l.code === code)?.label ?? code;
}

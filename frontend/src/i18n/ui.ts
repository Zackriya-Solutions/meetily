import { useSyncExternalStore } from 'react';

import i18n from './instance';

type EnglishTranslationBundle = {
  messages?: Record<string, string>;
};

/**
 * Translate interface text without touching meeting
 * content or backend identifiers.
 */
export function uiText(
  key: string,
  values?: Record<string, unknown>,
): string {
  return String(
    i18n.t(key, values ?? {}),
  );
}

/**
 * Subscribe React components to UI-language changes.
 */
const subscribe = (
  onChange: () => void,
): (() => void) => {
  const handleLanguageChanged = () => {
    onChange();
  };

  i18n.on(
    'languageChanged',
    handleLanguageChanged,
  );

  return () => {
    i18n.off(
      'languageChanged',
      handleLanguageChanged,
    );
  };
};

const snapshot = (): string =>
  i18n.resolvedLanguage ??
  i18n.language ??
  'en-US';

/**
 * Re-render components when the interface language changes.
 *
 * Useful for components that use uiText/uiLabel instead of
 * react-i18next's useTranslation().
 */
export function useUiTranslation(): string {
  return useSyncExternalStore(
    subscribe,
    snapshot,
    () => 'en-US',
  );
}

function getEnglishMessages():
  Record<string, string> {
  const bundle = i18n.getResourceBundle(
    'en-US',
    'translation',
  ) as EnglishTranslationBundle | undefined;

  return bundle?.messages ?? {};
}

/**
 * Translate known application labels returned by
 * native/model/settings APIs.
 *
 * Unknown text is preserved to avoid translating user
 * content, model names, paths and backend identifiers.
 */
export function uiLabel(
  text: string,
): string {
  if (!text) {
    return text;
  }

  const messages = getEnglishMessages();

  const entry = Object.entries(
    messages,
  ).find(([, value]) => value === text);

  if (!entry) {
    return text;
  }

  const [key] = entry;

  return uiText(
    `messages.${key}`,
  );
}

/**
 * Localize language display names only.
 * ISO/backend language codes remain untouched.
 */
export function languageLabel(
  code: string,
  fallback: string,
): string {
  if (!code) {
    return fallback;
  }

  if (
    code === 'auto' ||
    code === 'auto-translate'
  ) {
    return uiLabel(fallback);
  }

  const locale =
    i18n.resolvedLanguage ??
    i18n.language ??
    'en-US';

  const normalizedCode =
    normalizeLanguageCode(code);

  try {
    const displayNames =
      new Intl.DisplayNames(
        [locale],
        {
          type: 'language',
        },
      );

    return (
      displayNames.of(normalizedCode) ??
      fallback
    );
  } catch {
    return fallback;
  }
}

function normalizeLanguageCode(
  code: string,
): string {
  const normalized = code
    .trim()
    .replace(/_/g, '-')
    .toLowerCase();

  switch (normalized) {
    case 'zh-cn':
      return 'zh-Hans';

    case 'zh-tw':
    case 'zh-hk':
      return 'zh-Hant';

    case 'jw':
      return 'jv';

    default:
      return code.replace(/_/g, '-');
  }
}
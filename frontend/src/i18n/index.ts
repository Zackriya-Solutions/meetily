import i18n from './instance';

import {
  UI_LOCALES,
  SUPPORTED_UI_LANGUAGES,
  type UiLanguage,
} from './locales';

export const UI_LANGUAGE_STORAGE_KEY =
  'meetily.uiLanguage';

export function normalizeUiLanguage(
  language?: string | null,
): UiLanguage {
  if (!language) {
    return 'en-US';
  }

  const exact = SUPPORTED_UI_LANGUAGES.find(
    supported =>
      supported.toLowerCase() ===
      language.toLowerCase(),
  );

  if (exact) {
    return exact;
  }

  const normalized = language.toLowerCase();

  // Preserve Traditional Chinese where possible.
  if (
    normalized === 'zh-tw' ||
    normalized === 'zh-hk' ||
    normalized === 'zh-hant'
  ) {
    return 'zh-TW';
  }

  if (
    normalized === 'zh-cn' ||
    normalized === 'zh-sg' ||
    normalized === 'zh-hans' ||
    normalized === 'zh'
  ) {
    return 'zh-CN';
  }

  const languageCode =
    normalized.split('-')[0];

  const approximate =
    SUPPORTED_UI_LANGUAGES.find(
      supported =>
        supported
          .toLowerCase()
          .split('-')[0] === languageCode,
    );

  return approximate ?? 'en-US';
}

export function getPreferredUiLanguage(): UiLanguage {
  if (typeof window === 'undefined') {
    return 'en-US';
  }

  const storedLanguage =
    window.localStorage.getItem(
      UI_LANGUAGE_STORAGE_KEY,
    );

  if (
    storedLanguage &&
    SUPPORTED_UI_LANGUAGES.includes(
      storedLanguage as UiLanguage,
    )
  ) {
    return storedLanguage as UiLanguage;
  }

  return normalizeUiLanguage(
    window.navigator.language,
  );
}

export async function setUiLanguage(
  language: UiLanguage,
): Promise<void> {
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(
      UI_LANGUAGE_STORAGE_KEY,
      language,
    );

    document.documentElement.lang = language;
  }

  await i18n.changeLanguage(language);
}

export {
  UI_LOCALES,
  SUPPORTED_UI_LANGUAGES,
};

export type {
  UiLanguage,
};

export { i18n };

export default i18n;
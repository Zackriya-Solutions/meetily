import {
  en,
  zh,
} from '@blocknote/core/locales';

import i18n from './instance';

/**
 * Resolve BlockNote editor menus using the current UI
 * language without recreating the editor or replacing
 * unsaved document content.
 */
export const editorDictionary =
  new Proxy(en, {
    get(_target, property) {
      const language =
        i18n.resolvedLanguage ??
        i18n.language;

      const dictionary =
        language === 'zh-CN' ||
        language === 'zh-TW'
          ? zh
          : en;

      return Reflect.get(
        dictionary,
        property,
      );
    },
  });
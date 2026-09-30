import { createInstance } from 'i18next';
import { initReactI18next } from 'react-i18next';

import {
  UI_LOCALES,
  SUPPORTED_UI_LANGUAGES,
} from './locales';

const resources = Object.fromEntries(
  Object.entries(UI_LOCALES).map(
    ([language, config]) => [
      language,
      {
        translation: config.resource,
      },
    ],
  ),
);

const i18n = createInstance();

void i18n
  .use(initReactI18next)
  .init({
    resources,

    lng: 'en-US',
    fallbackLng: 'en-US',

    supportedLngs: SUPPORTED_UI_LANGUAGES,

    interpolation: {
      escapeValue: false,
    },
  });

export default i18n;
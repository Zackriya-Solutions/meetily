import enUS from './locales/en-US.json';
import zhCN from './locales/zh-CN.json';
import zhTW from './locales/zh-TW.json';
import jaJP from './locales/ja-JP.json';
import koKR from './locales/ko-KR.json';
import deDE from './locales/de-DE.json';
import frFR from './locales/fr-FR.json';
import esES from './locales/es-ES.json';
import ptBR from './locales/pt-BR.json';
import ruRU from './locales/ru-RU.json';

export const UI_LOCALES = {
  'en-US': {
    nativeName: 'English (US)',
    resource: enUS,
  },

  'zh-CN': {
    nativeName: '简体中文',
    resource: zhCN,
  },

  'zh-TW': {
    nativeName: '繁體中文',
    resource: zhTW,
  },

  'ja-JP': {
    nativeName: '日本語',
    resource: jaJP,
  },

  'ko-KR': {
    nativeName: '한국어',
    resource: koKR,
  },

  'de-DE': {
    nativeName: 'Deutsch',
    resource: deDE,
  },

  'fr-FR': {
    nativeName: 'Français',
    resource: frFR,
  },

  'es-ES': {
    nativeName: 'Español',
    resource: esES,
  },

  'pt-BR': {
    nativeName: 'Português (Brasil)',
    resource: ptBR,
  },

  'ru-RU': {
    nativeName: 'Русский',
    resource: ruRU,
  },
} as const;

export type UiLanguage = keyof typeof UI_LOCALES;

export const SUPPORTED_UI_LANGUAGES =
  Object.keys(UI_LOCALES) as UiLanguage[];
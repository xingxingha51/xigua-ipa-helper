import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

const languages = [
  ["az", "Azərbaycan"],
  ["en", "English"],
  ["am", "Հայերեն"],
  ["es", "Español"],
  ["it", "Italiano"],
  ["de", "Deutsch"],
  ["fr", "Français"],
  ["pl", "Polski"],
  ["nl", "Nederlands"],
  ["vi", "Tiếng Việt"],
  ["ru", "Русский"],
  ["ro", "Română"],
  ["ar", "العربية"],
  ["tr", "Türkçe"],
  ["zh_tw", "Traditional Chinese （繁體中文)"],
  ["zh_cn", "Simpified Chinese （简体中文)"],
  ["ko", "한국어"],
  ["zh_hk", "Cantonese （粵語)"],
  ["ja", "日本語"],
  ["cs_cz", "Čeština"],
  ["sv", "Svenska"],
  ["hu", "Magyar"],
  ["kh", "ភាសាខ្មែរ"],
  ["id", "Bahasa Indonesia"],
  ["pt_br", "Português (Brasileiro)"]
] as const;

export const sortedLanguages = [...languages].sort((a, b) =>
  a[0].localeCompare(b[0]),
);

type TranslationResource = Record<string, unknown>;

const localeModules = import.meta.glob<{ default: TranslationResource }>(
  "./locales/*.json",
  {
    eager: true,
  },
);

const resources = Object.fromEntries(
  Object.entries(localeModules).flatMap(([path, module]) => {
    const lang = path.match(/\/([\w-]+)\.json$/)?.[1];
    if (!lang) return [];

    return [[lang, { translation: module.default }]];
  }),
);

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    // Chinese first for this build's audience, then English. The second entry
    // is load-bearing: with zh_cn alone, any key it's missing "falls back" to
    // itself and renders as the raw key name (e.g. "device.pairing_cancel")
    // instead of readable English.
    fallbackLng: ["zh_cn", "en"],
    interpolation: {
      escapeValue: false,
    },
    resources,
  });

export default i18n;

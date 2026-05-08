import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './en.json';
import zh from './zh.json';

/** First-pass i18n: covers AppShell nav + a few high-traffic Settings
 * labels. Page-level strings will migrate over time. The default
 * language is whatever the user persists in settings; before that load
 * we initialise as English so first-paint is sensible. */
i18n
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      zh: { translation: zh },
    },
    lng: 'en',
    fallbackLng: 'en',
    interpolation: { escapeValue: false },
  })
  .catch(() => {});

export default i18n;

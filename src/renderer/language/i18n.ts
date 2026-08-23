import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import languages from './languages';

// react-i18next renamed reactI18nextModule to initReactI18next in v10.
i18n.use(initReactI18next).init({
	resources: languages,
	fallbackLng: 'en',
	debug: false,
	interpolation: {
		escapeValue: false, // React already escapes
	},
});

export default i18n;

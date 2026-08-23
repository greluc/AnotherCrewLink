import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';
import prettierPlugin from 'eslint-plugin-prettier';
import prettierConfig from 'eslint-config-prettier';
import globals from 'globals';

// Flat config, required from eslint 9 on. Replaces .eslintrc.yml.
export default tseslint.config(
	{ ignores: ['out/**', 'dist/**', 'node_modules/**', 'native/**'] },
	js.configs.recommended,
	...tseslint.configs.recommended,
	{
		...react.configs.flat.recommended,
		// Settings apply per config object in flat config, so the react plugin needs them
		// attached here rather than further down.
		settings: { react: { version: 'detect' } },
	},
	prettierConfig,
	{
		files: ['src/**/*.{ts,tsx}'],
		languageOptions: {
			ecmaVersion: 2022,
			sourceType: 'module',
			parserOptions: { ecmaFeatures: { jsx: true } },
			globals: { ...globals.browser, ...globals.node },
		},
		plugins: {
			prettier: prettierPlugin,
			// react-hooks was never wired up, which is why a conditionally called hook
			// sat in the overlay renderer undetected.
			'react-hooks': reactHooks,
		},
		rules: {
			'linebreak-style': ['error', 'unix'],
			'prettier/prettier': 'error',
			'@typescript-eslint/ban-ts-comment': 'off',
			'@typescript-eslint/no-non-null-assertion': 'off',
			'@typescript-eslint/no-unused-vars': [
				'error',
				{ argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
			],
			'react/react-in-jsx-scope': 'off',
			'react-hooks/rules-of-hooks': 'error',
			'react-hooks/exhaustive-deps': 'warn',
		},
	}
);

import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		include: ['src/**/*.test.ts'],
		// The modules under test are pure; anything touching Electron or the DOM needs
		// the app itself and is covered by the smoke run instead.
		environment: 'node',
	},
});

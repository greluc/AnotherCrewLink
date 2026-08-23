import { builtinModules, createRequire } from 'node:module';
import { resolve } from 'path';
import { defineConfig, externalizeDepsPlugin } from 'electron-vite';
import type { Plugin } from 'vite';
import react from '@vitejs/plugin-react';

const nodeRequire = createRequire(import.meta.url);

// The renderer runs with nodeIntegration, but Vite builds it as a browser target and
// replaces Node built-ins with empty stubs, which is what made `path.join` undefined.
// Electron exposes `require` on the renderer's window even from module scripts, so
// resolve these to a runtime lookup instead of bundling them.
const ELECTRON_EXPORTS = [
	'ipcRenderer',
	'shell',
	'webUtils',
	'clipboard',
	'contextBridge',
	'nativeImage',
	'webFrame',
	'desktopCapturer',
];

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
const RESERVED = new Set(['default', 'class', 'function', 'new', 'delete', 'import', 'export', 'const', 'let', 'var']);

function electronRendererNodeApis(): Plugin {
	const PREFIX = '\0electron-node:';
	const builtins = new Set(builtinModules);

	return {
		name: 'electron-renderer-node-apis',
		enforce: 'pre',
		resolveId(id) {
			const bare = id.startsWith('node:') ? id.slice(5) : id;
			if (bare === 'electron' || builtins.has(bare)) {
				return PREFIX + bare;
			}
			return null;
		},
		load(id) {
			if (!id.startsWith(PREFIX)) return null;
			const name = id.slice(PREFIX.length);

			let names: string[];
			if (name === 'electron') {
				// require('electron') from Node gives the binary path, not the module,
				// so this list cannot be introspected and is maintained by hand.
				names = ELECTRON_EXPORTS;
			} else {
				names = Object.keys(nodeRequire(name)).filter((key) => IDENTIFIER.test(key) && !RESERVED.has(key));
			}

			return [
				`const mod = window.require(${JSON.stringify(name)});`,
				'export default mod;',
				...names.map((key) => `export const ${key} = mod.${key};`),
			].join('\n');
		},
	};
}

export default defineConfig({
	main: {
		// Native modules and everything else from node_modules stay external so the
		// .node binaries are loaded from the app's node_modules at runtime.
		// electron-store and color are ESM-only, and the main bundle is CommonJS, so they
		// must be bundled rather than left as a runtime require().
		plugins: [externalizeDepsPlugin({ exclude: ['electron-store', 'color'] })],
		build: {
			rollupOptions: {
				input: resolve(__dirname, 'src/main/index.ts'),
			},
		},
	},
	renderer: {
		root: resolve(__dirname, 'src/renderer'),
		plugins: [electronRendererNodeApis(), react()],
		// The renderer is loaded over file:// in production, so asset URLs have to be
		// relative rather than rooted at /.
		base: './',
		build: {
			rollupOptions: {
				input: resolve(__dirname, 'src/renderer/index.html'),
			},
		},
	},
});

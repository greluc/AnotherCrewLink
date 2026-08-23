import { builtinModules, createRequire } from 'node:module';
import { resolve } from 'node:path';
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
		// The main bundle is ESM, so the ESM-only dependencies stay external instead of
		// being bundled through a CommonJS interop shim.
		plugins: [externalizeDepsPlugin()],
		build: {
			rollupOptions: {
				input: resolve(__dirname, 'src/main/index.ts'),
				// externalizeDepsPlugin covers plain dependencies. electron sits in
				// devDependencies and the vendored modules are file: paths, so neither is
				// caught, and both were being bundled. Their loaders use __dirname to find
				// their .node binaries, which does not exist in the ESM output.
				external: [
					'electron',
					'memoryjs',
					'node-keyboard-watcher',
					'electron-overlay-window',
					'structron',
					'registry-js',
				],
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

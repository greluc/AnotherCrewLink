/**
 * Turns `assets/icon.svg` into the raster files Windows needs, for both clients.
 *
 * The design system's mark is an SVG and Windows takes neither an SVG for a taskbar button
 * nor one for an executable's resource. So it is rendered once, here, and the results are
 * committed: `assets/icon.ico` for the three executables and `assets/icon.png` for the
 * window.
 *
 * Run it with:
 *
 *     npm run icon
 *
 * Inside Electron for the same reason `scripts/golden-vectors` is: Chromium is the renderer
 * the design system is drawn and reviewed in, so what ships is what the designer saw rather
 * than one rasteriser's opinion of the same file. It also costs no new dependency — the
 * only alternative in reach was adding an SVG rasteriser to the Rust build for a file that
 * changes about once a year.
 *
 * Each size is rendered from the vector rather than scaled down from the largest. A 16-pixel
 * icon downsampled from 256 is a grey smudge; one drawn at 16 has its strokes land on
 * pixels.
 */

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { app, BrowserWindow } from 'electron';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const source = join(repo, 'assets', 'icon.svg');

/**
 * The sizes Windows asks for.
 *
 * 16 and 32 are the taskbar and the Explorer list, 48 is the icon view, 256 is the extra
 * large view and the one the installer shows. 64 and 128 are the steps in between that stop
 * Windows scaling 48 up to 96 on a display at 200%.
 */
const SIZES = [16, 32, 48, 64, 128, 256];

/** Which size the window icon is taken at. Big enough for any display scaling to shrink. */
const WINDOW_ICON = 256;

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
	const window = new BrowserWindow({
		show: false,
		webPreferences: { nodeIntegration: true, contextIsolation: false, offscreen: true },
	});
	window.webContents.on('console-message', (_event, _level, message) => {
		console.log(`  page: ${message}`);
	});

	// As a data URI rather than a file the page fetches: an SVG loaded from `file:` taints
	// the canvas in some configurations, and a tainted canvas cannot be read back — which
	// would fail here as an empty result rather than as a permissions error.
	const svg = readFileSync(source);
	const uri = `data:image/svg+xml;base64,${svg.toString('base64')}`;

	await window.loadURL('data:text/html,<meta charset="utf-8">');
	const rendered = await window.webContents.executeJavaScript(`
		new Promise((resolve, reject) => {
			const image = new Image();
			image.onerror = () => reject(new Error('the SVG did not load'));
			image.onload = () => {
				const out = {};
				for (const size of ${JSON.stringify(SIZES)}) {
					const canvas = document.createElement('canvas');
					canvas.width = canvas.height = size;
					const context = canvas.getContext('2d');
					context.imageSmoothingQuality = 'high';
					context.drawImage(image, 0, 0, size, size);
					out[size] = canvas.toDataURL('image/png').split(',')[1];
				}
				resolve(out);
			};
			image.src = ${JSON.stringify(uri)};
		})
	`);

	const pngs = new Map(SIZES.map((size) => [size, Buffer.from(rendered[size], 'base64')]));
	for (const [size, bytes] of pngs) {
		if (bytes.length === 0) {
			console.error(`the ${size}px render came back empty`);
			app.exit(1);
			return;
		}
	}

	mkdirSync(join(repo, 'assets'), { recursive: true });

	const png = join(repo, 'assets', 'icon.png');
	writeFileSync(png, pngs.get(WINDOW_ICON));
	console.log(`assets/icon.png: ${WINDOW_ICON}x${WINDOW_ICON}, ${pngs.get(WINDOW_ICON).length} bytes`);

	const ico = join(repo, 'assets', 'icon.ico');
	writeFileSync(ico, buildIco(pngs));
	console.log(`assets/icon.ico: ${SIZES.join(', ')} at ${readFileSync(ico).length} bytes`);
	console.log(`  sha256 ${createHash('sha256').update(readFileSync(ico)).digest('hex')}`);

	// The 1.x app's icon, which electron-builder takes by convention: `electron-builder.yml`
	// sets `buildResources: resources`, and it reads `icon.ico` and `icon.png` out of there
	// without being told to. They held BetterCrewLink's artwork, inherited through the fork,
	// so the app that is still the one people use was introducing itself as the project it
	// forked from. Written from the same render as the 2.x client's, because the two are the
	// same program to anybody looking at a taskbar.
	writeFileSync(join(repo, 'resources', 'icon.ico'), readFileSync(ico));
	writeFileSync(join(repo, 'resources', 'icon.png'), readFileSync(png));
	console.log('resources/icon.ico, resources/icon.png: the same render, for electron-builder');

	app.exit(0);
});

/**
 * An ICO container holding one PNG per size.
 *
 * PNG payloads at every size rather than the older BMP-below-256 convention. Windows has
 * read PNG at any size since Vista and this project's floor is Windows 11, so the
 * convention buys nothing here and costs three times the bytes.
 *
 * @param {Map<number, Buffer>} images
 * @returns {Buffer}
 */
function buildIco(images) {
	const entries = [...images.entries()].sort((a, b) => a[0] - b[0]);
	const header = Buffer.alloc(6);
	header.writeUInt16LE(0, 0); // reserved
	header.writeUInt16LE(1, 2); // 1 = icon, 2 = cursor
	header.writeUInt16LE(entries.length, 4);

	const directory = Buffer.alloc(16 * entries.length);
	let offset = header.length + directory.length;
	entries.forEach(([size, bytes], index) => {
		const at = index * 16;
		// 256 is written as 0: the field is one byte and the format says zero means 256.
		directory.writeUInt8(size === 256 ? 0 : size, at);
		directory.writeUInt8(size === 256 ? 0 : size, at + 1);
		directory.writeUInt8(0, at + 2); // colours in the palette; 0 for true colour
		directory.writeUInt8(0, at + 3); // reserved
		directory.writeUInt16LE(1, at + 4); // colour planes
		directory.writeUInt16LE(32, at + 6); // bits per pixel
		directory.writeUInt32LE(bytes.length, at + 8);
		directory.writeUInt32LE(offset, at + 12);
		offset += bytes.length;
	});

	return Buffer.concat([header, directory, ...entries.map(([, bytes]) => bytes)]);
}

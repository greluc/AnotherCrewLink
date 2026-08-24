/**
 * Renders the Web Audio golden vectors that gate G2 measures the Rust DSP against.
 *
 * The reference is Chromium's own output, which is why this runs inside Electron rather
 * than against a specification read carefully: "parity" then stops being a matter of
 * opinion, and a disagreement is a bug in the port rather than a difference of reading.
 *
 * Run it with:
 *
 *     npm run golden
 *
 * It writes 32-bit float WAV files and a manifest into `test/golden/`, and prints what it
 * wrote. Re-running it must produce byte-identical files — every input is deterministic,
 * including the noise, which is why the generator carries its own seeded generator rather
 * than calling `Math.random`.
 */

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { app, BrowserWindow } from 'electron';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const outputDirectory = join(repo, 'test', 'golden');

/** The rate every vector is rendered at, and the one the client's context runs at. */
const SAMPLE_RATE = 48000;

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
	// Offscreen: this renders audio and writes files, and a window flashing up would be
	// the only visible part of it.
	const window = new BrowserWindow({
		show: false,
		webPreferences: {
			nodeIntegration: true,
			contextIsolation: false,
			offscreen: true,
		},
	});

	// Whatever the page logs, so a failure inside the generator is legible here rather
	// than as "an error was thrown, check the renderer console" with no renderer to check.
	window.webContents.on('console-message', (_event, _level, message, line, source) => {
		console.log(`  page ${source}:${line}: ${message}`);
	});

	await window.loadFile(join(here, 'host.html'));

	// The page reads the two assets itself. Passing 3.7 MB of samples as a JavaScript
	// literal is a 20 MB source string for `executeJavaScript` to parse, and it does not
	// survive it.
	const assets = {
		impulseResponse: join(repo, 'static', 'sounds', 'reverb.ogx'),
		recorded: join(repo, 'static', 'sounds', 'radio_static.wav'),
	};

	// The trailing semicolon comes off: the file is a statement, and wrapping a statement
	// in `(...)()` is a syntax error rather than a call.
	const generator = readFileSync(join(here, 'generate.js'), 'utf8')
		.trim()
		.replace(/;$/, '');
	let vectors;
	try {
		vectors = await window.webContents.executeJavaScript(
			`(${generator})(${SAMPLE_RATE}, ${JSON.stringify(assets)}).catch((error) => {
				console.error('generator:', error && error.stack ? error.stack : String(error));
				throw error;
			})`
		);
	} catch (error) {
		console.error('The generator failed:', error);
		app.exit(1);
		return;
	}

	// Cleared rather than merged: a vector that stops being generated must stop being
	// committed, or the next person measures against something nothing produces.
	rmSync(outputDirectory, { recursive: true, force: true });
	mkdirSync(outputDirectory, { recursive: true });

	const manifest = [];
	for (const vector of vectors) {
		const samples = Float32Array.from(vector.samples);
		const wav = encodeWav(samples, vector.channels, SAMPLE_RATE);
		const name = `${vector.name}.wav`;
		writeFileSync(join(outputDirectory, name), wav);
		manifest.push({
			name: vector.name,
			node: vector.node,
			input: vector.input,
			// Which input vector this one was rendered from, so the comparison does not
			// depend on the Rust side reproducing the input.
			from: vector.from,
			config: vector.config,
			channels: vector.channels,
			frames: samples.length / vector.channels,
			sha256: createHash('sha256').update(wav).digest('hex'),
		});
		console.log(`${name}: ${samples.length / vector.channels} frames, ${vector.channels}ch`);
	}

	writeFileSync(
		join(outputDirectory, 'manifest.json'),
		`${JSON.stringify({ sampleRate: SAMPLE_RATE, vectors: manifest }, null, '\t')}\n`
	);

	const total = readdirSync(outputDirectory).length;
	console.log(`\n${manifest.length} vectors, ${total} files in test/golden/`);
	app.exit(0);
});

/**
 * A 32-bit float WAV, which is what keeps a vector lossless.
 *
 * 16-bit would quantise the reference to about -96 dBFS, and the gate's tolerance is
 * -80 dBFS RMS error — close enough that the container would be part of the measurement.
 */
function encodeWav(samples, channels, sampleRate) {
	const bytesPerSample = 4;
	const header = Buffer.alloc(44);
	const dataLength = samples.length * bytesPerSample;

	header.write('RIFF', 0);
	header.writeUInt32LE(36 + dataLength, 4);
	header.write('WAVE', 8);
	header.write('fmt ', 12);
	header.writeUInt32LE(16, 16);
	// 3 is IEEE float, not 1 (PCM).
	header.writeUInt16LE(3, 20);
	header.writeUInt16LE(channels, 22);
	header.writeUInt32LE(sampleRate, 24);
	header.writeUInt32LE(sampleRate * channels * bytesPerSample, 28);
	header.writeUInt16LE(channels * bytesPerSample, 32);
	header.writeUInt16LE(8 * bytesPerSample, 34);
	header.write('data', 36);
	header.writeUInt32LE(dataLength, 40);

	const body = Buffer.alloc(dataLength);
	for (let index = 0; index < samples.length; index++) {
		body.writeFloatLE(samples[index], index * bytesPerSample);
	}
	return Buffer.concat([header, body]);
}

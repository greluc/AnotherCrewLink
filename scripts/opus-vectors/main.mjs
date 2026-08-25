/**
 * Captures Opus packets encoded by Chromium, for gate G2's fifth criterion.
 *
 * > Under a 5% loss profile with a Chromium sender, the Rust receive path recovers Opus
 * > in-band FEC.
 *
 * The obvious reading of "a Chromium sender" is a Chromium peer on the other end of a
 * connection, which would put this behind the whole transport layer. It does not have to
 * be: what the criterion needs from Chromium is *packets encoded the way Chromium encodes
 * them*, with the redundancy libwebrtc asks libopus for when it believes there is loss.
 * WebCodecs' `AudioEncoder` is that encoder, reachable from a page, and this is the same
 * trick `scripts/golden-vectors` already uses to make Chromium the reference for the DSP.
 *
 * What it cannot produce is the other half of criterion 5 — `getStats()` on an Electron
 * peer showing `fecPacketsSent` climbing — because that is a property of a peer connection
 * and there is no connection here. That half stays with P4. This half does not have to
 * wait for it.
 *
 * Run it with:
 *
 *     npm run opus-vectors
 *
 * It writes `test/opus/chromium-fec.bin` and a manifest, and prints what it wrote. Every
 * input is deterministic, so re-running must produce byte-identical output.
 */

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { app, BrowserWindow } from 'electron';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const outputDirectory = join(repo, 'test', 'opus');

const SAMPLE_RATE = 48000;
/** Opus's frame, and this client's. */
const FRAME_SAMPLES = 960;
/** Twenty seconds, which is long enough for the impairment model to be worth applying. */
const FRAMES = 1000;
/**
 * The loss the encoder is told about.
 *
 * Below roughly 5% libopus emits no usable redundancy at all — measured, and recorded in
 * §4.5 — so telling it about the profile's 5% is what the criterion is actually testing.
 */
const PACKET_LOSS_PERCENT = 5;

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
	const window = new BrowserWindow({
		show: false,
		webPreferences: { nodeIntegration: true, contextIsolation: false, offscreen: true },
	});
	// A file, not `about:blank`: WebCodecs is gated on a secure context.
	await window.loadFile(join(here, 'host.html'));

	const captured = await window.webContents.executeJavaScript(`
		(async () => {
			if (typeof AudioEncoder === 'undefined') {
				return { error: 'this Chromium has no WebCodecs AudioEncoder' };
			}

			const config = {
				codec: 'opus',
				sampleRate: ${SAMPLE_RATE},
				numberOfChannels: 1,
				bitrate: 32000,
				opus: {
					// The two that matter. Without the first there is no redundancy at all;
					// without the second libopus has no reason to spend bits on any.
					useinbandfec: true,
					packetlossperc: ${PACKET_LOSS_PERCENT},
					frameDuration: 20000,
				},
			};

			const support = await AudioEncoder.isConfigSupported(config);
			if (!support.supported) {
				return { error: 'Chromium refused the Opus configuration: ' + JSON.stringify(support.config) };
			}

			const packets = [];
			const encoder = new AudioEncoder({
				output: (chunk) => {
					const bytes = new Uint8Array(chunk.byteLength);
					chunk.copyTo(bytes);
					packets.push(Array.from(bytes));
				},
				error: (e) => { packets.error = String(e); },
			});
			encoder.configure(config);

			// Speech-like: a pitch that steps every frame, so concealment cannot imitate it
			// and a recovered frame is distinguishable from an invented one. The same
			// signal the Rust impairment harness uses, so the two are comparable.
			for (let frame = 0; frame < ${FRAMES}; frame += 1) {
				const hertz = 200 + (frame % 7) * 130;
				const samples = new Float32Array(${FRAME_SAMPLES});
				for (let index = 0; index < ${FRAME_SAMPLES}; index += 1) {
					samples[index] = Math.sin(2 * Math.PI * hertz * index / ${SAMPLE_RATE}) * 0.5;
				}
				const data = new AudioData({
					format: 'f32',
					sampleRate: ${SAMPLE_RATE},
					numberOfFrames: ${FRAME_SAMPLES},
					numberOfChannels: 1,
					timestamp: frame * 20000,
					data: samples,
				});
				encoder.encode(data);
				data.close();
			}
			await encoder.flush();
			encoder.close();
			return { packets };
		})()
	`);

	if (captured.error) {
		console.error(captured.error);
		app.exit(1);
		return;
	}

	// One file, length-prefixed: a u32 count, then each packet as a u32 length and its
	// bytes. Simple enough that the Rust side needs no parser and no dependency.
	const packets = captured.packets;
	let total = 4;
	for (const packet of packets) total += 4 + packet.length;
	const out = Buffer.alloc(total);
	out.writeUInt32LE(packets.length, 0);
	let at = 4;
	for (const packet of packets) {
		out.writeUInt32LE(packet.length, at);
		at += 4;
		Buffer.from(packet).copy(out, at);
		at += packet.length;
	}

	mkdirSync(outputDirectory, { recursive: true });
	const file = join(outputDirectory, 'chromium-fec.bin');
	writeFileSync(file, out);

	const digest = createHash('sha256').update(out).digest('hex');
	const sizes = packets.map((p) => p.length);
	writeFileSync(
		join(outputDirectory, 'manifest.json'),
		`${JSON.stringify(
			{
				source: 'Chromium WebCodecs AudioEncoder, via Electron',
				chromium: process.versions.chrome,
				electron: process.versions.electron,
				sampleRate: SAMPLE_RATE,
				frameSamples: FRAME_SAMPLES,
				packets: packets.length,
				packetLossPercentToldToTheEncoder: PACKET_LOSS_PERCENT,
				smallestPacket: Math.min(...sizes),
				largestPacket: Math.max(...sizes),
				sha256: digest,
			},
			null,
			'\t'
		)}\n`
	);

	console.log(`wrote ${packets.length} packets, ${out.length} bytes`);
	console.log(`chromium ${process.versions.chrome}`);
	console.log(`sha256 ${digest}`);
	app.exit(0);
});

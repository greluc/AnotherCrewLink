/**
 * Does a Chromium receiver recover *our* in-band FEC?
 *
 * Gate G2's fifth criterion, fourth leg. The other three are settled by inspecting
 * packets: Chromium emits redundancy, our receiver recovers it, and we emit redundancy.
 * This one cannot be settled that way, because what is in question is not our bytes but
 * what Chromium's decoder does with them.
 *
 * It had been parked behind the transport layer on the grounds that Chromium has to
 * receive our stream and there is nothing to carry it. There is: an encoded transform on
 * the sender can *replace* a frame's payload, not only drop it. So Chromium packetises our
 * Opus, sends it to itself, and its own receive path — NetEQ, libopus, its FEC recovery —
 * decodes it.
 *
 * The loss goes in on the **receiving** side, after depacketisation and before NetEQ.
 * Dropped on the sending side it would happen before packetisation, the sequence numbers
 * would close up, and nothing downstream would ever know a frame was missing. That is the
 * mistake `scripts/receive-reference` documents and this one had to avoid.
 *
 * Two runs, because one number is not a measurement: the same audio encoded with the
 * redundancy and without it. If Chromium conceals less from the first, it recovered ours.
 *
 * Run it with:
 *
 *     npm run our-fec
 *
 * `cargo test -p acl-audio --test write_our_vectors -- --ignored` writes the input.
 */

import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { app, BrowserWindow } from 'electron';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const vectors = join(repo, 'test', 'opus');

/** The profile the criterion names. */
const LOSS_PERCENT = 5;
const SECONDS = 10;

/** Reads the length-prefixed packet file the Rust side writes. */
function packetsFrom(name) {
	const raw = readFileSync(join(vectors, name));
	const count = raw.readUInt32LE(0);
	const packets = [];
	let at = 4;
	for (let index = 0; index < count; index += 1) {
		const length = raw.readUInt32LE(at);
		at += 4;
		packets.push(Array.from(raw.subarray(at, at + length)));
		at += length;
	}
	return packets;
}

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
	let withFec;
	let withoutFec;
	try {
		withFec = packetsFrom('ours-fec.bin');
		withoutFec = packetsFrom('ours-nofec.bin');
	} catch (error) {
		console.error(
			`could not read the vectors (${error.message}). Run:\n` +
				'  cargo test -p acl-audio --test write_our_vectors -- --ignored'
		);
		app.exit(1);
		return;
	}

	const window = new BrowserWindow({
		show: false,
		webPreferences: { nodeIntegration: true, contextIsolation: false, offscreen: true },
	});
	await window.loadFile(join(here, 'host.html'));

	const result = await window.webContents.executeJavaScript(
		`window.compare(${JSON.stringify(withFec)}, ${JSON.stringify(withoutFec)}, ${LOSS_PERCENT}, ${SECONDS})`,
		true
	);

	if (result.error) {
		console.error(result.error);
		app.exit(1);
		return;
	}

	const { protectedRun, bareRun } = result;
	console.log(`chromium ${process.versions.chrome}, ${LOSS_PERCENT}% loss on the receiving side\n`);
	for (const [label, run] of [
		['our packets WITH redundancy', protectedRun],
		['the same audio WITHOUT it ', bareRun],
	]) {
		console.log(
			`${label}  concealed ${run.concealedShare.toFixed(2)}%  ` +
				`(${run.concealedSamples} of ${run.totalSamplesReceived} samples, ` +
				`${run.droppedCount} frames dropped, ${run.packetsReceived} packets)`
		);
	}

	const recovered = bareRun.concealedShare - protectedRun.concealedShare;
	console.log(`\ndifference: ${recovered.toFixed(2)} percentage points`);
	console.log(
		recovered > 0.1
			? '=> CHROMIUM RECOVERED OUR REDUNDANCY'
			: '=> no measurable recovery'
	);

	mkdirSync(join(repo, 'test', 'receive'), { recursive: true });
	writeFileSync(
		join(repo, 'test', 'receive', 'our-fec.json'),
		`${JSON.stringify(
			{
				source: 'Chromium loopback; our Opus substituted into the sender, loss injected on the receiver',
				chromium: process.versions.chrome,
				lossPercent: LOSS_PERCENT,
				seconds: SECONDS,
				withRedundancy: protectedRun,
				withoutRedundancy: bareRun,
			},
			null,
			'\t'
		)}\n`
	);
	app.exit(recovered > 0.1 ? 0 : 1);
});

/**
 * Measures Chromium's own receive path under the impairment profiles, for gate G2's third
 * criterion.
 *
 * > Under each impairment profile, the Rust receive path's added mouth-to-ear latency is
 * > within 30 ms of Chromium's and its objective quality score is no more than 0.2 MOS
 * > below it.
 *
 * "Chromium's" was read as "a Chromium peer over a network", which put the criterion
 * behind the transport layer. It does not have to be. A loopback `RTCPeerConnection` is
 * Chromium's real receive path — NetEQ, its delay manager, its concealment, all of it —
 * and an **encoded transform** on the sender is a place to apply the impairment before the
 * packets reach it. The connection is local, so what the receiver sees is exactly and only
 * the loss this script injects.
 *
 * The profiles and the generator are the same ones `acl-audio::impairment` uses, down to
 * the seed, so both sides drop the same packets. `chromium_reference.rs` checks that claim
 * rather than trusting it: the dropped sequence numbers are written out and compared
 * against what the Rust model produces.
 *
 * Run it with:
 *
 *     npm run receive-reference
 *
 * It takes about two minutes, because a peer connection runs in real time.
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { app, BrowserWindow } from 'electron';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const outputDirectory = join(repo, 'test', 'receive');

/** How long each profile runs. Long enough for NetEQ's delay manager to settle. */
const SECONDS = 8;

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
	const window = new BrowserWindow({
		show: false,
		webPreferences: { nodeIntegration: true, contextIsolation: false, offscreen: true },
	});
	await window.loadFile(join(here, 'host.html'));

	const measured = await window.webContents.executeJavaScript(
		`window.measureAll(${SECONDS})`,
		true
	);

	if (measured.error) {
		console.error(measured.error);
		app.exit(1);
		return;
	}

	mkdirSync(outputDirectory, { recursive: true });
	writeFileSync(
		join(outputDirectory, 'chromium.json'),
		`${JSON.stringify(
			{
				source: 'Chromium loopback RTCPeerConnection with an encoded transform',
				chromium: process.versions.chrome,
				electron: process.versions.electron,
				seconds: SECONDS,
				profiles: measured.profiles,
			},
			null,
			'\t'
		)}\n`
	);

	console.log(`chromium ${process.versions.chrome}`);
	if (measured.diagnostics) console.log('letzter lauf:', JSON.stringify(measured.diagnostics));
	console.log('profile         jitter ms   concealed   fecSent   dropped');
	for (const p of measured.profiles) {
		console.log(
			`${p.name.padEnd(14)} ${String(p.jitterBufferMs).padStart(9)} ${`${p.concealedShare.toFixed(1)}%`.padStart(11)} ${String(p.fecPacketsSent).padStart(9)} ${String(p.dropped.length).padStart(9)}`
		);
	}
	app.exit(0);
});

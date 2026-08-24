import { app } from 'electron';
import { createWriteStream, mkdirSync, type WriteStream } from 'node:fs';
import { join } from 'node:path';
import type { AmongUsState } from '../common/AmongUsState';

/**
 * Records what the reader read, and what it made of it.
 *
 * Gate G1 of the Rust port asks that the Rust reader produce the same `AmongUsState` as
 * this one for every recorded frame, field for field. That is only checkable against
 * frames from a real game — a fixture written by hand would only prove the two
 * implementations share an author's assumptions.
 *
 * So this captures both halves at once: every region the reader touched, and the state it
 * produced from them. Replaying the regions into the Rust reader and comparing the states
 * is the whole of the gate.
 *
 * # Cost when it is off
 *
 * One boolean per read. `recording` is checked before anything is allocated, and the
 * default is off, so a normal session pays a branch on a call that already crosses into
 * native code.
 *
 * # Turning it on
 *
 * Set `ACL_RECORD` to a name before starting the app:
 *
 * ```text
 * set ACL_RECORD=polus-tasks
 * ```
 *
 * The file lands in `userData/recordings/<name>.ndjson`, one JSON object per frame.
 */

/** One region the reader read, as it was at that moment. */
interface RecordedRead {
	/** Where it started, as hex so a 64-bit address survives JSON. */
	a: string;
	/** What was there, base64. */
	b: string;
}

/** One frame: what was read, and what the reader concluded. */
interface RecordedFrame {
	frame: number;
	/** Whether the game process is 64-bit, which decides pointer width on replay. */
	is64: boolean;
	/** The module the offsets are relative to. */
	module?: { name: string; base: string; size: string };
	reads: RecordedRead[];
	state: AmongUsState;
}

let stream: WriteStream | undefined;
let recording = false;
let frame = 0;
let reads: RecordedRead[] = [];
/** Deduplicates within a frame: the reader re-reads the same address many times. */
const seen = new Set<string>();

/** Whether anything is being recorded. Checked on every read, so it stays a bare boolean. */
export function isRecording(): boolean {
	return recording;
}

/**
 * Starts recording to `userData/recordings/<name>.ndjson`.
 *
 * Called from the main process at start-up when `ACL_RECORD` is set. Never throws: a
 * recorder that takes the app down with it is worse than one that does not record.
 */
export function startRecording(name: string): string | undefined {
	try {
		const directory = join(app.getPath('userData'), 'recordings');
		mkdirSync(directory, { recursive: true });
		const path = join(directory, `${name.replace(/[^\w.-]/g, '_')}.ndjson`);
		stream = createWriteStream(path, { flags: 'a' });
		recording = true;
		frame = 0;
		console.log('Recording game reads to', path);
		return path;
	} catch (error) {
		console.error('Could not start recording:', error);
		recording = false;
		return undefined;
	}
}

/** Stops recording and closes the file. */
export function stopRecording(): void {
	recording = false;
	stream?.end();
	stream = undefined;
}

/**
 * Notes one region the reader read.
 *
 * Called from the read primitives rather than from the call sites, so a region cannot be
 * missed by someone adding a read later — which would show up as an unexplained parity
 * failure rather than as a missing recording.
 */
export function noteRead(address: number, bytes: Buffer): void {
	if (!recording || bytes.length === 0) {
		return;
	}
	// A frame re-reads the same address many times: the same pointer chain is walked for
	// every field hanging off it. Keeping the first is enough, because nothing writes to
	// the game between reads within one frame.
	const key = `${address}:${bytes.length}`;
	if (seen.has(key)) {
		return;
	}
	seen.add(key);
	reads.push({ a: `0x${address.toString(16)}`, b: bytes.toString('base64') });
}

/**
 * Ends a frame, writing what was read and what came of it.
 *
 * A frame with no reads is not written: the reader idles when no game is open, and a file
 * full of empty frames makes the interesting ones harder to find.
 */
export function endFrame(
	state: AmongUsState,
	is64: boolean,
	module?: { name: string; base: number; size: number }
): void {
	if (!recording) {
		return;
	}
	if (reads.length === 0) {
		seen.clear();
		return;
	}
	const record: RecordedFrame = {
		frame: frame++,
		is64,
		module: module
			? {
					name: module.name,
					base: `0x${module.base.toString(16)}`,
					size: `0x${module.size.toString(16)}`,
				}
			: undefined,
		reads,
		// Structured-cloned rather than referenced: the reader mutates its state object
		// between frames, and a reference would make every recorded frame the last one.
		state: JSON.parse(JSON.stringify(state)) as AmongUsState,
	};
	reads = [];
	seen.clear();
	try {
		stream?.write(`${JSON.stringify(record)}\n`);
	} catch (error) {
		console.error('Could not write a recorded frame:', error);
	}
}

/** Starts recording if the environment asks for it. Called once, at start-up. */
export function startRecordingIfAsked(): void {
	const name = process.env.ACL_RECORD;
	if (name) {
		startRecording(name);
	}
}

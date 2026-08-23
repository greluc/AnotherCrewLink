/**
 * Writes what the app logs to a file under userData, so a problem a player hits once
 * during a match can be looked at afterwards.
 *
 * Without this the console output only exists in a window nobody has open: reports
 * arrive as descriptions of symptoms, and the state that produced them is gone.
 */

import { appendFileSync, mkdirSync, renameSync, statSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';
import { app } from 'electron';

const MAX_BYTES = 4 * 1024 * 1024;

let logPath: string | undefined;
let failed = false;

export function logDirectory(): string {
	return join(app.getPath('userData'), 'logs');
}

function currentPath(): string | undefined {
	if (failed) return undefined;
	if (!logPath) {
		try {
			mkdirSync(logDirectory(), { recursive: true });
			logPath = join(logDirectory(), 'anothercrewlink.log');
		} catch {
			// Logging must never be the reason the app does not start.
			failed = true;
			return undefined;
		}
	}
	return logPath;
}

/** Keeps one previous file, so a long session cannot fill the disk. */
function rotateIfLarge(path: string): void {
	try {
		if (statSync(path).size < MAX_BYTES) return;
		const previous = `${path}.1`;
		try {
			unlinkSync(previous);
		} catch {
			// there was no previous file
		}
		renameSync(path, previous);
	} catch {
		// the file does not exist yet, which is the normal case on the first write
	}
}

export function writeLog(source: string, level: string, message: string): void {
	const path = currentPath();
	if (!path) return;
	const line = `${new Date().toISOString()} [${source}/${level}] ${message}\n`;
	try {
		rotateIfLarge(path);
		appendFileSync(path, line);
	} catch {
		failed = true;
	}
}

/** Mirrors the main process console into the same file. */
export function captureMainConsole(): void {
	const levels: Array<'log' | 'info' | 'warn' | 'error'> = ['log', 'info', 'warn', 'error'];
	for (const level of levels) {
		const original = console[level].bind(console);
		console[level] = (...args: unknown[]) => {
			original(...args);
			writeLog(
				'main',
				level,
				args
					.map((arg) => {
						if (typeof arg === 'string') return arg;
						if (arg instanceof Error) return arg.stack ?? arg.message;
						try {
							return JSON.stringify(arg);
						} catch {
							return String(arg);
						}
					})
					.join(' ')
			);
		};
	}
}

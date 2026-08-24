import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { GameState, type AmongUsState } from '../common/AmongUsState';

// The recorder asks Electron where userData lives. Point it at a temporary directory so
// the test writes real files through the real stream — the point of this file is the
// bytes that reach disk, so stubbing the filesystem would test nothing.
let userData = '';
vi.mock('electron', () => ({
	app: { getPath: () => userData },
}));

const { endFrame, isRecording, noteRead, startRecording, stopRecording } = await import('./recorder');

/** Where the committed format fixture lives, shared with the Rust side. */
const FIXTURE_DIRECTORY = join(dirname(fileURLToPath(import.meta.url)), '../../test/fixtures/recording-format');

function state(overrides: Partial<AmongUsState> = {}): AmongUsState {
	return {
		gameState: GameState.LOBBY,
		lobbyCode: 'ABCDEF',
		players: [],
		isHost: false,
		hostId: 0,
		clientId: 0,
		comsSabotaged: false,
		currentCamera: 0,
		map: 0,
		closedDoors: [],
		...overrides,
	} as AmongUsState;
}

function frames(path: string): Record<string, unknown>[] {
	return readFileSync(path, 'utf8')
		.split('\n')
		.filter((line) => line.length > 0)
		.map((line) => JSON.parse(line) as Record<string, unknown>);
}

describe('recorder', () => {
	beforeEach(() => {
		userData = mkdtempSync(join(tmpdir(), 'acl-recorder-'));
	});

	afterEach(async () => {
		await stopRecording();
		rmSync(userData, { recursive: true, force: true });
	});

	it('writes nothing until it is asked to', async () => {
		expect(isRecording()).toBe(false);
		noteRead(0x1000, Buffer.from([1, 2, 3]));
		endFrame(state(), false);
		// No stream was ever opened, so there is no file to find.
		expect(() => frames(join(userData, 'recordings', 'x.ndjson'))).toThrow();
	});

	it('records an address as hex and its bytes as base64', async () => {
		const path = startRecording('one') as string;
		noteRead(0x14000abcd, Buffer.from([0x4d, 0x5a, 0x90]));
		endFrame(state(), true, { name: 'GameAssembly.dll', base: 0x140000000, size: 0x4000 });
		await stopRecording();

		const [first] = frames(path);
		expect(first.frame).toBe(0);
		expect(first.is64).toBe(true);
		expect(first.module).toEqual({ name: 'GameAssembly.dll', base: '0x140000000', size: '0x4000' });
		expect(first.reads).toEqual([{ a: '0x14000abcd', b: 'TVqQ' }]);
	});

	it('deduplicates within a frame but not across frames', async () => {
		// The reader walks the same pointer chain for every field hanging off it, so one
		// frame re-reads the same address many times. Across frames it must record again:
		// the value has changed, and that is the whole point of a frame.
		const path = startRecording('two') as string;
		noteRead(0x1000, Buffer.from([1]));
		noteRead(0x1000, Buffer.from([1]));
		endFrame(state(), false);
		noteRead(0x1000, Buffer.from([2]));
		endFrame(state(), false);
		await stopRecording();

		const written = frames(path);
		expect(written).toHaveLength(2);
		expect(written[0].reads).toHaveLength(1);
		expect(written[1].reads).toEqual([{ a: '0x1000', b: 'Ag==' }]);
		expect(written[1].frame).toBe(1);
	});

	it('copies the state instead of referencing it', async () => {
		// The reader mutates one state object in place between frames. A reference would
		// make every recorded frame equal to the last one — and the recording would look
		// plausible while being worthless.
		const path = startRecording('three') as string;
		const live = state({ lobbyCode: 'FIRST' });
		noteRead(0x1000, Buffer.from([1]));
		endFrame(live, false);
		live.lobbyCode = 'SECOND';
		noteRead(0x2000, Buffer.from([2]));
		endFrame(live, false);
		await stopRecording();

		const written = frames(path);
		expect((written[0].state as AmongUsState).lobbyCode).toBe('FIRST');
		expect((written[1].state as AmongUsState).lobbyCode).toBe('SECOND');
	});

	it('skips a frame that read nothing', async () => {
		// The reader idles when no game is open. A file full of empty frames buries the
		// interesting ones.
		const path = startRecording('four') as string;
		endFrame(state(), false);
		noteRead(0x1000, Buffer.from([1]));
		endFrame(state(), false);
		await stopRecording();

		const written = frames(path);
		expect(written).toHaveLength(1);
		expect(written[0].frame).toBe(0);
	});

	it('produces the fixture the Rust parity harness parses', async () => {
		// Gate G1's replay reads this format. Writing the fixture from the real recorder,
		// and parsing it from the real Rust harness, is what stops a recording session
		// being lost to a format change that neither side noticed.
		const path = startRecording('format') as string;
		noteRead(0x140001000, Buffer.from([0x4d, 0x5a, 0x00, 0x00]));
		noteRead(0x140002000, Buffer.from([0xef, 0xbe, 0xad, 0xde]));
		endFrame(state({ lobbyCode: 'FORMAT' }), true, {
			name: 'GameAssembly.dll',
			base: 0x140000000,
			size: 0x10000,
		});
		await stopRecording();

		mkdirSync(FIXTURE_DIRECTORY, { recursive: true });
		writeFileSync(join(FIXTURE_DIRECTORY, 'one-frame.ndjson'), readFileSync(path));
		expect(frames(path)).toHaveLength(1);
	});
});

/**
 * Renders one golden vector per node, configuration and input, inside Chromium.
 *
 * Loaded as a string and evaluated in the offscreen window, so it is a single expression:
 * an async arrow that takes the sample rate and the paths of the two audio assets, and
 * returns plain objects the main process writes to disk. It reads the assets itself, over
 * `nodeIntegration`, because handing them across as a literal is a 20 MB source string.
 *
 * Every input is deterministic. The noise carries its own generator rather than calling
 * `Math.random`, because a vector that changes between runs is not a reference.
 */
async (sampleRate, assets) => {
	const { readFileSync } = require('node:fs');
	/** Most vectors are this long. Enough to settle, short enough to commit. */
	const SHORT = 0.5;

	/** The convolver needs room for the tail, or the vector ends mid-reverb. */
	const LONG = 2.0;

	/** The panner settings the client creates every peer with. */
	const PANNER = {
		panningModel: 'equalpower',
		distanceModel: 'linear',
		refDistance: 0.1,
		rolloffFactor: 1,
		maxDistance: 5.32,
	};

	/**
	 * A 32-bit xorshift, so the noise is the same on every machine and every run.
	 *
	 * `Math.random` is seeded per process and would make each run a different reference.
	 */
	function noise(length, seed) {
		const out = new Float32Array(length);
		let state = seed >>> 0;
		for (let index = 0; index < length; index++) {
			state ^= state << 13;
			state >>>= 0;
			state ^= state >>> 17;
			state ^= state << 5;
			state >>>= 0;
			// Signed, centred, and just under full scale so nothing clips before a node
			// has had a chance to.
			out[index] = (state / 0x8000_0000 - 1) * 0.9;
		}
		return out;
	}

	/** A single sample at 1.0, which is what makes a node's impulse response readable. */
	function impulse(length) {
		const out = new Float32Array(length);
		out[0] = 1;
		return out;
	}

	/** A logarithmic sweep from 20 Hz to the Nyquist, for frequency response. */
	function sweep(length, rate) {
		const out = new Float32Array(length);
		const start = 20;
		const end = rate / 2;
		const duration = length / rate;
		const ratio = Math.log(end / start);
		for (let index = 0; index < length; index++) {
			const t = index / rate;
			const phase = ((2 * Math.PI * start * duration) / ratio) * (Math.exp((t / duration) * ratio) - 1);
			out[index] = Math.sin(phase) * 0.9;
		}
		return out;
	}

	function load(path) {
		const bytes = readFileSync(path);
		// A copy, because `decodeAudioData` detaches the buffer it is given and Node's
		// Buffer views a pooled allocation that other reads share.
		return new Uint8Array(bytes).buffer;
	}

	const context = new OfflineAudioContext(1, 1, sampleRate);
	const impulseResponse = await context.decodeAudioData(load(assets.impulseResponse));
	const recordedBuffer = await context.decodeAudioData(load(assets.recorded));

	/** An excerpt of the real recording, so one input is not synthetic. */
	function recorded(length) {
		const source = recordedBuffer.getChannelData(0);
		const out = new Float32Array(length);
		// From one second in, past whatever silence the file starts with.
		const offset = Math.min(sampleRate, Math.max(0, source.length - length));
		for (let index = 0; index < length; index++) {
			out[index] = source[(offset + index) % source.length];
		}
		return out;
	}

	const inputs = {
		impulse: (rate, seconds) => impulse(Math.round(rate * seconds)),
		noise: (rate, seconds) => noise(Math.round(rate * seconds), 0x1234_5678),
		sweep: (rate, seconds) => sweep(Math.round(rate * seconds), rate),
		recorded: (rate, seconds) => recorded(Math.round(rate * seconds)),
	};

	/** Renders one graph and returns its interleaved samples. */
	async function render(seconds, channels, build) {
		const frames = Math.round(sampleRate * seconds);
		const offline = new OfflineAudioContext(channels, frames, sampleRate);
		build(offline, frames);
		const rendered = await offline.startRendering();

		const out = new Float32Array(rendered.length * rendered.numberOfChannels);
		for (let channel = 0; channel < rendered.numberOfChannels; channel++) {
			const data = rendered.getChannelData(channel);
			for (let frame = 0; frame < rendered.length; frame++) {
				out[frame * rendered.numberOfChannels + channel] = data[frame];
			}
		}
		return out;
	}

	/** A buffer source over one of the deterministic inputs. */
	function source(offline, frames, input) {
		const buffer = offline.createBuffer(1, frames, sampleRate);
		buffer.copyToChannel(inputs[input](sampleRate, frames / sampleRate), 0);
		const node = offline.createBufferSource();
		node.buffer = buffer;
		return node;
	}

	const vectors = [];
	const writtenInputs = new Set();

	/**
	 * Writes the input itself as a vector, once per input and duration.
	 *
	 * Without this the Rust side would have to regenerate the inputs from the same
	 * formulas, and a disagreement about the *input* would read as a disagreement about
	 * the node. Handing over both halves makes the comparison about the node alone.
	 */
	function addInput(input, seconds) {
		const name = `input__${input}__${seconds}s`;
		if (writtenInputs.has(name)) return name;
		writtenInputs.add(name);
		const frames = Math.round(sampleRate * seconds);
		vectors.push({
			name,
			node: 'input',
			input,
			config: { seconds },
			channels: 1,
			samples: [...inputs[input](sampleRate, seconds)],
		});
		return name;
	}

	async function add(node, input, config, seconds, channels, build) {
		const label = Object.entries(config)
			.map(([key, value]) => `${key}-${value}`)
			.join('_');
		const name = `${node}__${input}${label ? `__${label}` : ''}`;
		const from = addInput(input, seconds);
		const samples = await render(seconds, channels, (offline, frames) =>
			build(offline, source(offline, frames, input))
		);
		vectors.push({ name, node, input, from, config, channels, samples: [...samples] });
	}

	// ---------------------------------------------------------------- gain

	// Every gain the voice decision can produce: silence, the vent and camera gains, and
	// full scale. A trivial node, and the one that proves the harness itself is honest.
	for (const value of [0, 0.5, 0.8, 1]) {
		await add('gain', 'noise', { value }, SHORT, 1, (offline, input) => {
			const gain = offline.createGain();
			gain.gain.value = value;
			input.connect(gain).connect(offline.destination);
			input.start();
		});
	}

	// ---------------------------------------------------------------- biquad

	// The three the client actually sets, plus the shape a node is created with.
	const filters = [
		{ type: 'lowpass', frequency: 350, Q: 1 },
		{ type: 'lowpass', frequency: 2000, Q: 20 },
		{ type: 'lowpass', frequency: 2300, Q: -15 },
		{ type: 'highpass', frequency: 1000, Q: 10 },
	];
	for (const filter of filters) {
		for (const input of ['impulse', 'sweep', 'noise']) {
			await add('biquad', input, filter, SHORT, 1, (offline, node) => {
				const biquad = offline.createBiquadFilter();
				biquad.type = filter.type;
				biquad.frequency.value = filter.frequency;
				biquad.Q.value = filter.Q;
				node.connect(biquad).connect(offline.destination);
				node.start();
			});
		}
	}

	// ---------------------------------------------------------------- panner

	// Where a peer can be: on top of the listener, to each side, diagonally away, and at
	// the edge of the range. Z is -0.5 because the client puts every peer slightly in
	// front, which is what stops a peer at the listener's exact position being ambiguous.
	const positions = [
		{ x: 0, y: 0, z: -0.5 },
		{ x: 1, y: 0, z: -0.5 },
		{ x: -1, y: 0, z: -0.5 },
		{ x: 3, y: 3, z: -0.5 },
		{ x: 5.32, y: 0, z: -0.5 },
	];
	for (const position of positions) {
		for (const input of ['impulse', 'noise']) {
			await add('panner', input, position, SHORT, 2, (offline, node) => {
				const panner = offline.createPanner();
				panner.panningModel = PANNER.panningModel;
				panner.distanceModel = PANNER.distanceModel;
				panner.refDistance = PANNER.refDistance;
				panner.rolloffFactor = PANNER.rolloffFactor;
				panner.maxDistance = PANNER.maxDistance;
				panner.positionX.setValueAtTime(position.x, 0);
				panner.positionY.setValueAtTime(position.y, 0);
				panner.positionZ.setValueAtTime(position.z, 0);
				node.connect(panner).connect(offline.destination);
				node.start();
			});
		}
	}

	// ---------------------------------------------------------------- convolver

	// The impulse response itself, decoded, so the Rust side does not need an Ogg
	// decoder to check the convolver. It is the same buffer the node was given, which
	// makes the comparison about the convolution rather than about two decoders.
	{
		const channels = impulseResponse.numberOfChannels;
		const frames = impulseResponse.length;
		const interleaved = new Float32Array(frames * channels);
		for (let channel = 0; channel < channels; channel++) {
			const data = impulseResponse.getChannelData(channel);
			for (let frame = 0; frame < frames; frame++) {
				interleaved[frame * channels + channel] = data[frame];
			}
		}
		vectors.push({
			name: 'impulse-response',
			node: 'impulse-response',
			input: 'reverb.ogx',
			config: { sampleRate: impulseResponse.sampleRate },
			channels,
			samples: [...interleaved],
		});
	}


	// The reverb an impostor hears a haunting ghost through. Rendered long enough to
	// carry the tail: a vector that ends mid-reverb would make a truncated port look
	// correct. `normalize` is left at its default, which is what the client leaves it at
	// and is where the specification's normalisation scalar comes from.
	for (const input of ['impulse', 'recorded']) {
		await add('convolver', input, { normalize: true }, LONG, 2, (offline, node) => {
			const convolver = offline.createConvolver();
			convolver.buffer = impulseResponse;
			node.connect(convolver).connect(offline.destination);
			node.start();
		});
	}

	// ---------------------------------------------------------------- chain

	// The whole per-peer path as the client builds it: source into panner into gain, with
	// the muffle in place. One vector that would catch an error in how the nodes compose
	// even if each of them matched on its own.
	await add(
		'chain',
		'recorded',
		{ muffle: 'lowpass-2000', gain: 0.5, x: 1 },
		SHORT,
		2,
		(offline, node) => {
			const panner = offline.createPanner();
			panner.panningModel = PANNER.panningModel;
			panner.distanceModel = PANNER.distanceModel;
			panner.refDistance = PANNER.refDistance;
			panner.rolloffFactor = PANNER.rolloffFactor;
			panner.maxDistance = PANNER.maxDistance;
			panner.positionX.setValueAtTime(1, 0);
			panner.positionY.setValueAtTime(0, 0);
			panner.positionZ.setValueAtTime(-0.5, 0);

			const muffle = offline.createBiquadFilter();
			muffle.type = 'lowpass';
			muffle.frequency.value = 2000;
			muffle.Q.value = 20;

			const gain = offline.createGain();
			gain.gain.value = 0.5;

			node.connect(panner).connect(muffle).connect(gain).connect(offline.destination);
			node.start();
		}
	);

	return vectors;
};

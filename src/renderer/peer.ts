/**
 * Minimal WebRTC peer wrapper, replacing simple-peer.
 *
 * simple-peer was last published in January 2023 and pulled in seven dependencies
 * (buffer, readable-stream, randombytes and friends) to wrap APIs the renderer has
 * natively. This covers exactly the surface this app used: an initiator/answerer
 * pair that exchanges opaque signal blobs through the socket server, one audio
 * stream each way, and one data channel for lobby settings and impostor radio.
 */

export type SignalData =
	| { type: 'offer'; sdp: string }
	| { type: 'answer'; sdp: string }
	| { candidate: RTCIceCandidateInit }
	| { renegotiate: true };

export interface PeerOptions {
	initiator?: boolean;
	stream?: MediaStream;
	config?: RTCConfiguration;
	/** Overrides how long the connection may take to come up. */
	connectTimeout?: number;
}

/**
 * A connection that never completes is not reported as failed by anything: an offer
 * that no one answers leaves the connection in `new`, where it stays for as long as
 * the app runs, because ICE never starts and so can never fail. That is one of the
 * ways a player ends up unable to hear exactly one other, with a restart as the only
 * way out. Give up on our own and let the caller rebuild instead.
 *
 * Setting up a peer connection normally takes under a second; this only has to
 * outlast a slow relay allocation.
 */
export const PEER_CONNECT_TIMEOUT = 20000;

type Handler = (...args: never[]) => void;

const DATA_CHANNEL_LABEL = 'anothercrewlink';

export default class Peer {
	private pc: RTCPeerConnection;
	private channel?: RTCDataChannel;
	private handlers = new Map<string, Set<Handler>>();
	private destroyed = false;
	/** Candidates that arrive before the remote description is applied. */
	private pendingCandidates: RTCIceCandidateInit[] = [];
	private remoteDescriptionSet = false;
	private connectEmitted = false;

	private connectTimer?: ReturnType<typeof setTimeout>;

	constructor(private options: PeerOptions = {}) {
		this.pc = new RTCPeerConnection(options.config);

		this.connectTimer = setTimeout(() => {
			this.connectTimer = undefined;
			if (this.destroyed || this.connectEmitted) return;
			this.emit('error', new Error('peer connection did not come up in time'));
			this.destroy();
		}, options.connectTimeout ?? PEER_CONNECT_TIMEOUT);

		if (options.stream) {
			for (const track of options.stream.getTracks()) {
				this.pc.addTrack(track, options.stream);
			}
		}

		this.pc.onicecandidate = (event) => {
			if (event.candidate) {
				this.emit('signal', { candidate: event.candidate.toJSON() });
			}
		};

		this.pc.ontrack = (event) => {
			const [stream] = event.streams;
			if (stream) this.emit('stream', stream);
		};

		this.pc.onconnectionstatechange = () => {
			const state = this.pc.connectionState;
			if (state === 'connected') {
				// Audio is flowing. The data channel opening is what emits connect, and it
				// normally follows immediately, but a connection carrying voice must never be
				// torn down for being slow to finish that.
				this.clearConnectTimer();
			}
			if (state === 'failed') {
				this.emit('error', new Error(`peer connection ${state}`));
				this.destroy();
			} else if (state === 'closed') {
				this.destroy();
			}
		};

		if (options.initiator) {
			// Only the initiator creates the channel; the answerer picks it up via
			// ondatachannel, which mirrors how simple-peer negotiated it.
			this.setChannel(this.pc.createDataChannel(DATA_CHANNEL_LABEL));
			this.pc.onnegotiationneeded = () => {
				void this.negotiate();
			};
		} else {
			this.pc.ondatachannel = (event) => this.setChannel(event.channel);
		}
	}

	/** True once the data channel can accept a send(), matching simple-peer. */
	get writable(): boolean {
		return !this.destroyed && this.channel?.readyState === 'open';
	}

	get connected(): boolean {
		return this.writable;
	}

	on(event: string, handler: Handler): this {
		if (!this.handlers.has(event)) this.handlers.set(event, new Set());
		this.handlers.get(event)!.add(handler);
		return this;
	}

	off(event: string, handler: Handler): this {
		this.handlers.get(event)?.delete(handler);
		return this;
	}

	send(data: string): void {
		if (this.channel?.readyState === 'open') {
			this.channel.send(data);
		}
	}

	/** Feed in a blob that the remote peer produced via its 'signal' event. */
	async signal(data: SignalData): Promise<void> {
		if (this.destroyed) return;
		try {
			if ('candidate' in data) {
				if (this.remoteDescriptionSet) {
					await this.pc.addIceCandidate(data.candidate);
				} else {
					this.pendingCandidates.push(data.candidate);
				}
				return;
			}
			if ('renegotiate' in data) {
				if (this.options.initiator) await this.negotiate();
				return;
			}

			await this.pc.setRemoteDescription({ type: data.type, sdp: data.sdp });
			this.remoteDescriptionSet = true;
			// Candidates may legitimately arrive before the description does.
			const queued = this.pendingCandidates.splice(0);
			for (const candidate of queued) {
				await this.pc.addIceCandidate(candidate).catch(() => {
					/* a stale candidate is not fatal */
				});
			}

			if (data.type === 'offer') {
				const answer = await this.pc.createAnswer();
				await this.pc.setLocalDescription(answer);
				this.emit('signal', { type: 'answer', sdp: this.pc.localDescription!.sdp });
			}
		} catch (error) {
			this.emit('error', error instanceof Error ? error : new Error(String(error)));
		}
	}

	private clearConnectTimer(): void {
		if (this.connectTimer !== undefined) {
			clearTimeout(this.connectTimer);
			this.connectTimer = undefined;
		}
	}

	destroy(): void {
		if (this.destroyed) return;
		this.destroyed = true;
		this.clearConnectTimer();
		try {
			this.channel?.close();
		} catch {
			/* already gone */
		}
		try {
			this.pc.onicecandidate = null;
			this.pc.ontrack = null;
			this.pc.onconnectionstatechange = null;
			this.pc.onnegotiationneeded = null;
			this.pc.ondatachannel = null;
			this.pc.close();
		} catch {
			/* already gone */
		}
		this.emit('close');
		this.handlers.clear();
	}

	private setChannel(channel: RTCDataChannel): void {
		this.channel = channel;
		channel.onopen = () => {
			if (!this.connectEmitted) {
				this.connectEmitted = true;
				this.clearConnectTimer();
				this.emit('connect');
			}
		};
		channel.onmessage = (event) => this.emit('data', event.data);
		channel.onclose = () => this.destroy();
		channel.onerror = () => {
			/* onconnectionstatechange reports the failure that matters */
		};
	}

	private async negotiate(): Promise<void> {
		if (this.destroyed) return;
		try {
			const offer = await this.pc.createOffer();
			await this.pc.setLocalDescription(offer);
			this.emit('signal', { type: 'offer', sdp: this.pc.localDescription!.sdp });
		} catch (error) {
			this.emit('error', error instanceof Error ? error : new Error(String(error)));
		}
	}

	private emit(event: string, ...args: unknown[]): void {
		for (const handler of this.handlers.get(event) ?? []) {
			try {
				(handler as (...a: unknown[]) => void)(...args);
			} catch (error) {
				console.error(`peer handler for "${event}" threw`, error);
			}
		}
	}
}

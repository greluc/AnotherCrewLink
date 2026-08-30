// Self-contained NAT/TURN probe for aucl.greluc.me. No dependencies at all: the
// Socket.IO handshake is done by hand over Engine.IO v4 on a raw WebSocket, because
// installing this project's client pulls three native modules that need a Windows
// toolchain, and the point is to run this from somewhere else entirely.
//
// Run: node probe.mjs [https://aucl.greluc.me]
import dgram from 'node:dgram';
import net from 'node:net';
import tls from 'node:tls';
import crypto from 'node:crypto';
import dns from 'node:dns/promises';

const BASE = (process.argv[2] ?? 'https://aucl.greluc.me').replace(/\/$/, '');

// --- STUN / TURN ------------------------------------------------------------

const MAGIC = 0x2112a442;
const BINDING_REQ = 0x0001;
const BINDING_ERR = 0x0111;
const ALLOCATE_REQ = 0x0003;
const ALLOCATE_OK = 0x0103;
const REFRESH_REQ = 0x0004;
const CREATE_PERMISSION = 0x0008;
const CREATE_PERMISSION_OK = 0x0108;
const SEND_INDICATION = 0x0016;
const DATA_INDICATION = 0x0017;
const A = {
	MAPPED_ADDRESS: 0x0001,
	USERNAME: 0x0006,
	MESSAGE_INTEGRITY: 0x0008,
	ERROR_CODE: 0x0009,
	LIFETIME: 0x000d,
	REALM: 0x0014,
	NONCE: 0x0015,
	XOR_RELAYED_ADDRESS: 0x0016,
	REQUESTED_TRANSPORT: 0x0019,
	XOR_MAPPED_ADDRESS: 0x0020,
	SOFTWARE: 0x8022,
	OTHER_ADDRESS: 0x802c,
	CHANGE_REQUEST: 0x0003,
	XOR_PEER_ADDRESS: 0x0012,
	DATA: 0x0013,
};

const pad4 = (n) => (4 - (n % 4)) % 4;

function attr(type, value) {
	const v = Buffer.isBuffer(value) ? value : Buffer.from(value);
	const b = Buffer.alloc(4 + v.length + pad4(v.length));
	b.writeUInt16BE(type, 0);
	b.writeUInt16BE(v.length, 2);
	v.copy(b, 4);
	return b;
}

function header(type, txn, bodyLen) {
	const b = Buffer.alloc(20);
	b.writeUInt16BE(type, 0);
	b.writeUInt16BE(bodyLen, 2);
	b.writeUInt32BE(MAGIC, 4);
	txn.copy(b, 8);
	return b;
}

const txnId = () => crypto.randomBytes(12);

// MESSAGE-INTEGRITY is an HMAC-SHA1 over the message as it would look with the
// attribute already appended -- the header length counts it -- but computed over the
// bytes before it. Getting that wrong yields a 401 that looks like bad credentials.
function build(type, txn, attrs, creds) {
	let body = Buffer.concat(attrs);
	if (!creds) return Buffer.concat([header(type, txn, body.length), body]);
	const partial = Buffer.concat([header(type, txn, body.length + 24), body]);
	const key = crypto.createHash('md5').update(`${creds.username}:${creds.realm}:${creds.password}`, 'utf8').digest();
	body = Buffer.concat([body, attr(A.MESSAGE_INTEGRITY, crypto.createHmac('sha1', key).update(partial).digest())]);
	return Buffer.concat([header(type, txn, body.length), body]);
}

function parse(buf) {
	if (buf.length < 20) return null;
	const len = buf.readUInt16BE(2);
	if (buf.length < 20 + len) return null;
	const attrs = new Map();
	let off = 20;
	while (off + 4 <= 20 + len) {
		const t = buf.readUInt16BE(off);
		const l = buf.readUInt16BE(off + 2);
		if (off + 4 + l > 20 + len) break;
		if (!attrs.has(t)) attrs.set(t, buf.subarray(off + 4, off + 4 + l));
		off += 4 + l + pad4(l);
	}
	return { type: buf.readUInt16BE(0), txn: buf.subarray(8, 20), attrs };
}

function xorAddr(v, txn) {
	if (!v || v.length < 8) return null;
	const port = v.readUInt16BE(2) ^ 0x2112;
	const cookie = Buffer.alloc(4);
	cookie.writeUInt32BE(MAGIC, 0);
	if (v.readUInt8(1) === 0x01) {
		const ip = [];
		for (let i = 0; i < 4; i++) ip.push(v[4 + i] ^ cookie[i]);
		return { address: ip.join('.'), port };
	}
	const mask = Buffer.concat([cookie, txn]);
	const parts = [];
	for (let i = 0; i < 8; i++) parts.push((((v[4 + i * 2] ^ mask[i * 2]) << 8) | (v[5 + i * 2] ^ mask[i * 2 + 1])).toString(16));
	return { address: parts.join(':'), port };
}

function xorAddrValue(address, port) {
	const b = Buffer.alloc(8);
	b.writeUInt8(0x01, 1);
	b.writeUInt16BE(port ^ 0x2112, 2);
	const cookie = Buffer.alloc(4);
	cookie.writeUInt32BE(MAGIC, 0);
	address.split('.').forEach((o, i) => b.writeUInt8(Number(o) ^ cookie[i], 4 + i));
	return b;
}

function plainAddr(v) {
	if (!v || v.length < 8 || v.readUInt8(1) !== 0x01) return null;
	return { address: `${v[4]}.${v[5]}.${v[6]}.${v[7]}`, port: v.readUInt16BE(2) };
}

const errorCode = (v) => (v && v.length >= 4 ? { code: v[2] * 100 + v[3], reason: v.subarray(4).toString('utf8') } : null);
const fmt = (a) => (a ? `${a.address}:${a.port}` : 'none');

function parseIceUrl(url) {
	const m = /^(stun|stuns|turn|turns):([^:?/]+)(?::(\d+))?(?:\?(.*))?$/i.exec(String(url).trim());
	if (!m) return null;
	const scheme = m[1].toLowerCase();
	const transport = (new URLSearchParams(m[4] ?? '').get('transport') ?? '').toLowerCase();
	const secure = scheme === 'turns' || scheme === 'stuns';
	return {
		url: String(url),
		scheme,
		host: m[2],
		port: m[3] ? Number(m[3]) : secure ? 5349 : 3478,
		transport: transport || (secure ? 'tcp' : 'udp'),
		secure,
		isRelay: scheme === 'turn' || scheme === 'turns',
	};
}

class UdpTransport {
	constructor(host, port) {
		this.host = host;
		this.port = port;
		this.socket = dgram.createSocket('udp4');
		this.waiters = [];
		this.extra = [];
		this.socket.on('error', () => {});
		this.socket.on('message', (msg) => {
			const p = parse(msg);
			if (!p) return;
			for (const fn of this.extra) fn(p);
			for (let i = this.waiters.length - 1; i >= 0; i--) {
				if (this.waiters[i].txn.equals(p.txn)) {
					const [w] = this.waiters.splice(i, 1);
					clearTimeout(w.timer);
					w.resolve(p);
				}
			}
		});
	}
	ready() {
		return new Promise((res, rej) => {
			this.socket.once('error', rej);
			this.socket.bind(0, res);
		});
	}
	// Retransmits: one lost datagram must not read as a blocked relay.
	request(msg, txn, { timeout = 1500, attempts = 3, host = this.host, port = this.port } = {}) {
		return new Promise((resolve) => {
			let tries = 0;
			const w = { txn, resolve, timer: null };
			this.waiters.push(w);
			const fire = () => {
				if (++tries > attempts) {
					const i = this.waiters.indexOf(w);
					if (i >= 0) this.waiters.splice(i, 1);
					return resolve(null);
				}
				this.socket.send(msg, port, host, () => {});
				w.timer = setTimeout(fire, timeout);
			};
			fire();
		});
	}
	push(msg) {
		this.socket.send(msg, this.port, this.host, () => {});
	}
	close() {
		for (const w of this.waiters.splice(0)) clearTimeout(w.timer);
		try {
			this.socket.close();
		} catch {}
	}
}

class StreamTransport {
	constructor(stream) {
		this.stream = stream;
		this.buf = Buffer.alloc(0);
		this.waiters = [];
		this.extra = [];
		stream.on('error', () => {});
		stream.on('data', (chunk) => {
			this.buf = Buffer.concat([this.buf, chunk]);
			for (;;) {
				if (this.buf.length < 20) break;
				const total = 20 + this.buf.readUInt16BE(2);
				if (this.buf.length < total) break;
				const p = parse(this.buf.subarray(0, total));
				this.buf = this.buf.subarray(total);
				if (!p) break;
				for (const fn of this.extra) fn(p);
				for (let i = this.waiters.length - 1; i >= 0; i--) {
					if (this.waiters[i].txn.equals(p.txn)) {
						const [w] = this.waiters.splice(i, 1);
						clearTimeout(w.timer);
						w.resolve(p);
					}
				}
			}
		});
	}
	request(msg, txn, { timeout = 6000 } = {}) {
		return new Promise((resolve) => {
			const w = { txn, resolve, timer: null };
			this.waiters.push(w);
			w.timer = setTimeout(() => {
				const i = this.waiters.indexOf(w);
				if (i >= 0) this.waiters.splice(i, 1);
				resolve(null);
			}, timeout);
			this.stream.write(msg);
		});
	}
	push(msg) {
		this.stream.write(msg);
	}
	close() {
		for (const w of this.waiters.splice(0)) clearTimeout(w.timer);
		try {
			this.stream.destroy();
		} catch {}
	}
}

async function connectTransport(server, timeout = 8000) {
	if (server.transport === 'udp') {
		const t = new UdpTransport(server.host, server.port);
		await t.ready();
		return t;
	}
	const stream = await new Promise((resolve, reject) => {
		const timer = setTimeout(() => reject(new Error(`connect timed out after ${timeout}ms`)), timeout);
		const onErr = (e) => {
			clearTimeout(timer);
			reject(e);
		};
		// SNI matters: a relay behind a shared address needs it to present the right cert.
		const s = server.secure
			? tls.connect({ host: server.host, port: server.port, servername: server.host }, () => (clearTimeout(timer), s.off('error', onErr), resolve(s)))
			: net.connect({ host: server.host, port: server.port }, () => (clearTimeout(timer), s.off('error', onErr), resolve(s)));
		s.on('error', onErr);
	});
	return new StreamTransport(stream);
}

async function binding(transport, extra = [], options = {}) {
	const txn = txnId();
	const res = await transport.request(build(BINDING_REQ, txn, extra), txn, options);
	if (!res) return { ok: false, reason: 'no response' };
	if (res.type === BINDING_ERR) {
		const e = errorCode(res.attrs.get(A.ERROR_CODE));
		return { ok: false, reason: `${e?.code} ${e?.reason}` };
	}
	return {
		ok: true,
		mapped: xorAddr(res.attrs.get(A.XOR_MAPPED_ADDRESS), res.txn) ?? plainAddr(res.attrs.get(A.MAPPED_ADDRESS)),
		other: plainAddr(res.attrs.get(A.OTHER_ADDRESS)),
		software: res.attrs.get(A.SOFTWARE)?.toString('utf8'),
	};
}

// Unauthenticated Allocate to learn realm and nonce, then the signed one. 438 Stale
// Nonce is retried with the nonce the server just handed back: ordinary traffic.
async function allocate(transport, { username, password }) {
	const reqTransport = attr(A.REQUESTED_TRANSPORT, Buffer.from([17, 0, 0, 0]));
	const txn1 = txnId();
	const probe = await transport.request(build(ALLOCATE_REQ, txn1, [reqTransport]), txn1);
	if (!probe) return { ok: false, stage: 'challenge', reason: 'no response to the unauthenticated Allocate' };
	const realm = probe.attrs.get(A.REALM)?.toString('utf8');
	let nonce = probe.attrs.get(A.NONCE);
	if (probe.type === ALLOCATE_OK) return { ok: true, openRelay: true, relayed: xorAddr(probe.attrs.get(A.XOR_RELAYED_ADDRESS), probe.txn) };
	if (!realm || !nonce) {
		const e = errorCode(probe.attrs.get(A.ERROR_CODE));
		return { ok: false, stage: 'challenge', reason: `expected 401 with realm and nonce, got ${e ? `${e.code} ${e.reason}` : `type 0x${probe.type.toString(16)}`}` };
	}
	for (let i = 0; i < 2; i++) {
		const txn = txnId();
		const attrs = [
			reqTransport,
			attr(A.USERNAME, Buffer.from(username, 'utf8')),
			attr(A.REALM, Buffer.from(realm, 'utf8')),
			attr(A.NONCE, nonce),
		];
		const res = await transport.request(build(ALLOCATE_REQ, txn, attrs, { username, password, realm }), txn);
		if (!res) return { ok: false, stage: 'allocate', reason: 'no response to the authenticated Allocate', realm };
		if (res.type === ALLOCATE_OK) {
			return {
				ok: true,
				relayed: xorAddr(res.attrs.get(A.XOR_RELAYED_ADDRESS), res.txn),
				mapped: xorAddr(res.attrs.get(A.XOR_MAPPED_ADDRESS), res.txn),
				lifetime: res.attrs.get(A.LIFETIME)?.readUInt32BE(0),
				software: res.attrs.get(A.SOFTWARE)?.toString('utf8'),
				realm,
				nonce,
			};
		}
		const e = errorCode(res.attrs.get(A.ERROR_CODE));
		const fresh = res.attrs.get(A.NONCE);
		if (e?.code === 438 && fresh) {
			nonce = fresh;
			continue;
		}
		return { ok: false, stage: 'allocate', reason: `${e?.code} ${e?.reason || '(no reason text)'}`, realm };
	}
	return { ok: false, stage: 'allocate', reason: 'stale nonce twice', realm };
}

async function permission(transport, alloc, creds, address, port) {
	let nonce = alloc.nonce;
	for (let i = 0; i < 2; i++) {
		const txn = txnId();
		const msg = build(
			CREATE_PERMISSION,
			txn,
			[
				attr(A.XOR_PEER_ADDRESS, xorAddrValue(address, port)),
				attr(A.USERNAME, Buffer.from(creds.username, 'utf8')),
				attr(A.REALM, Buffer.from(alloc.realm, 'utf8')),
				attr(A.NONCE, nonce),
			],
			{ username: creds.username, password: creds.password, realm: alloc.realm }
		);
		const res = await transport.request(msg, txn, { timeout: 3000, attempts: 3 });
		if (!res) return 'no response';
		if (res.type === CREATE_PERMISSION_OK) {
			alloc.nonce = nonce;
			return 'GRANTED';
		}
		const e = errorCode(res.attrs.get(A.ERROR_CODE));
		const fresh = res.attrs.get(A.NONCE);
		if (e?.code === 438 && fresh) {
			nonce = fresh;
			continue;
		}
		return `refused ${e?.code} ${e?.reason || '(no reason text)'}`;
	}
	return 'stale nonce twice';
}

async function release(transport, alloc, creds) {
	if (!alloc.realm || !alloc.nonce) return;
	const txn = txnId();
	await transport.request(
		build(
			REFRESH_REQ,
			txn,
			[
				attr(A.LIFETIME, Buffer.alloc(4)),
				attr(A.USERNAME, Buffer.from(creds.username, 'utf8')),
				attr(A.REALM, Buffer.from(alloc.realm, 'utf8')),
				attr(A.NONCE, alloc.nonce),
			],
			{ username: creds.username, password: creds.password, realm: alloc.realm }
		),
		txn,
		{ timeout: 2000, attempts: 1 }
	);
}

// --- Socket.IO by hand, over Engine.IO v4 long-polling ----------------------

// The server refuses Engine.IO long-polling (HTTP 400, code 3) and serves only
// WebSocket, which is also the single transport Voice.tsx asks for. Node has had a
// global WebSocket since 22, so this still needs nothing installed.
function fetchPeerConfig(base) {
	return new Promise((resolve, reject) => {
		const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/socket.io/?EIO=4&transport=websocket`);
		let sid;
		const timer = setTimeout(() => {
			try {
				ws.close();
			} catch {}
			reject(new Error('no clientPeerConfig within 20s'));
		}, 20000);
		ws.onerror = () => {
			clearTimeout(timer);
			reject(new Error('websocket failed to connect'));
		};
		ws.onmessage = (ev) => {
			const p = String(ev.data);
			// '0' open, '2' ping, '40' namespace connect (+ack), '42' event.
			if (p.startsWith('0') && !p.startsWith('40')) {
				ws.send('40');
				return;
			}
			if (p === '2') {
				ws.send('3');
				return;
			}
			if (p.startsWith('40') && p.length > 2) {
				sid = JSON.parse(p.slice(2)).sid;
				return;
			}
			if (!p.startsWith('42')) return;
			const [event, payload] = JSON.parse(p.slice(2));
			if (event !== 'clientPeerConfig') return;
			clearTimeout(timer);
			try {
				ws.close();
			} catch {}
			resolve({ sid, config: payload });
		};
	});
}

// --- run --------------------------------------------------------------------

const line = (s = '') => console.log(s);

line(`## 0. Where this is running from`);
const [{ address: serverIp }] = await dns.lookup(new URL(BASE).hostname, { all: true, family: 4 });
line(`   signalling host ${new URL(BASE).hostname} resolves to ${serverIp}`);

line('');
line(`## 1. clientPeerConfig from ${BASE}`);
const { sid, config } = await fetchPeerConfig(BASE);
// Shortened on purpose: this runs in public CI logs on a public repository, and the
// socket id is the second half of the TURN username. Enough to correlate with a server
// log, not enough to reconstruct the credential.
line(`   socket id ${String(sid).slice(0, 4)}...`);
line(`   forceRelayOnly: ${config.forceRelayOnly}`);
for (const s of config.iceServers) {
	const urls = [].concat(s.urls).map(String).join(' , ');
	// The expiry half of the username is the useful half and is not a secret; the rest is
	// the socket id again. The credential itself is never printed, only its length.
	const user = s.username ? String(s.username).replace(/^([^:]*):.*$/, '$1:<redacted>') : '';
	line(`   ${urls}${s.username ? `  user=${user} credential=<${String(s.credential).length} chars>` : ''}`);
}

line('');
line('## 2. This machine behind whatever NAT it has');
const shared = new UdpTransport('', 0);
await shared.ready();
line(`   local port ${shared.socket.address().port}, one socket for every server below`);
const mappings = [];
for (const [host, port] of [
	['stun.l.google.com', 19302],
	['stun1.l.google.com', 19302],
	['stun.cloudflare.com', 3478],
	['stun.nextcloud.com', 3478],
	['stun.sipgate.net', 3478],
	[new URL(BASE).hostname, 3478],
]) {
	const r = await binding(shared, [], { host, port, timeout: 2000, attempts: 3 });
	line(`   ${host}:${port} -> ${r.ok ? fmt(r.mapped) : r.reason}${r.other ? ` [OTHER-ADDRESS ${fmt(r.other)}]` : ''}`);
	if (r.ok && r.mapped) mappings.push({ host, port, ...r });
}
// A STUN server on the same LAN reports the private address and would otherwise read
// as a second, different mapping -- i.e. as symmetric NAT that is not there.
const isPrivate = (a) => /^(10\.|127\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.)/.test(a);
const external = mappings.filter((m) => !isPrivate(m.mapped.address));
for (const m of mappings.filter((x) => isPrivate(x.mapped.address))) {
	line(`   (${m.host} is on this machine's own network -- ${fmt(m.mapped)} left out of the verdict)`);
}
const distinct = new Set(external.map((m) => fmt(m.mapped)));
const distinctHosts = new Set(external.map((m) => m.host)).size;
line(
	`   mapping: ${
		distinctHosts < 2
			? 'undetermined, fewer than two servers answered'
			: distinct.size === 1
				? `ENDPOINT-INDEPENDENT (${[...distinct][0]} everywhere) - direct connections are possible`
				: `ADDRESS-DEPENDENT (${[...distinct].join(' | ')}) - symmetric NAT, only a relay works`
	}`
);
const localPort = shared.socket.address().port;
const ownPublic = external.find((m) => m.mapped)?.mapped?.address;
line(`   public address of this machine: ${ownPublic ?? 'unknown'}${ownPublic === serverIp ? '  <-- SAME AS THE SERVER, this run is not from outside' : '  <-- different from the server, this run is from outside'}`);
line(`   port preserved by the NAT: ${mappings.some((m) => m.mapped.port === localPort) ? 'yes' : 'no'}`);

const rfc5780 = mappings.find((m) => m.other);
if (rfc5780) {
	for (const [label, flags] of [
		['different address and port', 0x00000006],
		['same address, different port', 0x00000002],
	]) {
		const change = Buffer.alloc(4);
		change.writeUInt32BE(flags, 0);
		const r = await binding(shared, [attr(A.CHANGE_REQUEST, change)], { host: rfc5780.host, port: rfc5780.port, timeout: 2000, attempts: 2 });
		line(`   filtering, ${label}: ${r.ok ? 'reply arrived' : 'no reply'}`);
	}
} else {
	line('   filtering: not probed, no server offered OTHER-ADDRESS');
}
shared.close();

line('');
line('## 3. Every advertised endpoint');
const relays = [];
for (const server of config.iceServers) {
	for (const url of [].concat(server.urls).map(String)) {
		const t = parseIceUrl(url);
		if (!t) {
			line(`   ${url}: UNPARSEABLE`);
			continue;
		}
		line('');
		line(`   ${url}`);
		let transport;
		try {
			transport = await connectTransport(t);
		} catch (e) {
			line(`      transport: FAILED - ${e.message}`);
			continue;
		}
		const b = await binding(transport, []);
		line(`      STUN Binding: ${b.ok ? `ok, mapped ${fmt(b.mapped)}${b.software ? ` (${b.software})` : ''}` : b.reason}`);
		if (!t.isRelay || !server.username) {
			transport.close();
			continue;
		}
		const creds = { username: server.username, password: server.credential };
		const alloc = await allocate(transport, creds);
		if (!alloc.ok) {
			line(`      TURN Allocate: FAILED at ${alloc.stage} - ${alloc.reason}`);
			transport.close();
			continue;
		}
		line(`      TURN Allocate: OK - relayed ${fmt(alloc.relayed)}, lifetime ${alloc.lifetime}s, realm "${alloc.realm}"${alloc.software ? `, ${alloc.software}` : ''}`);
		relays.push({ t, transport, alloc, creds, server });
	}
}

line('');
line('## 4. Peer policy - which peer addresses the relay will talk to');
line('   (the question a run from the server\'s own network cannot answer:');
line('    is the relay\'s own address denied, or is the caller\'s address denied?)');
const first = relays[0];
if (!first) {
	line('   skipped, no allocation to ask with');
} else {
	const probes = [
		['1.1.1.1', 3478, 'an unrelated public address'],
		['8.8.8.8', 3478, 'another unrelated public address'],
		[serverIp, 49999, "the relay's OWN public address - what relay-to-relay needs"],
		[ownPublic ?? '203.0.113.9', 49999, `this machine's own public address`],
		['203.0.113.9', 3478, 'a documentation-range address'],
		['10.1.0.30', 3478, 'a private address'],
		['127.0.0.1', 3478, 'loopback'],
	];
	for (const [addr, port, note] of probes) {
		line(`   ${addr}:${port} (${note}): ${await permission(first.transport, first.alloc, first.creds, addr, port)}`);
	}
}

line('');
line('## 5. Does the relay actually forward? A STUN request pushed through it.');
for (const r of relays) {
	const [peerIp] = await dns.resolve4('stun.cloudflare.com');
	const perm = await permission(r.transport, r.alloc, r.creds, peerIp, 3478);
	if (perm !== 'GRANTED') {
		line(`   ${r.t.url}: permission for ${peerIp} ${perm}`);
		continue;
	}
	const got = new Promise((resolve) => {
		const timer = setTimeout(() => resolve(null), 6000);
		r.transport.extra.push((p) => {
			if (p.type !== DATA_INDICATION) return;
			clearTimeout(timer);
			resolve({ from: xorAddr(p.attrs.get(A.XOR_PEER_ADDRESS), p.txn), data: p.attrs.get(A.DATA) });
		});
	});
	const innerTxn = txnId();
	r.transport.push(
		build(SEND_INDICATION, txnId(), [attr(A.XOR_PEER_ADDRESS, xorAddrValue(peerIp, 3478)), attr(A.DATA, build(BINDING_REQ, innerTxn, []))])
	);
	const reply = await got;
	if (!reply) {
		line(`   ${r.t.url}: NOTHING CAME BACK - the relay did not forward`);
		continue;
	}
	const inner = parse(reply.data);
	const mapped = inner ? xorAddr(inner.attrs.get(A.XOR_MAPPED_ADDRESS), inner.txn) : null;
	const matches = mapped && mapped.address === r.alloc.relayed.address && mapped.port === r.alloc.relayed.port;
	line(`   ${r.t.url}: reply from ${fmt(reply.from)}, far end saw ${fmt(mapped)} - ${matches ? 'MATCHES the relayed address, traffic really went through' : 'does NOT match the relayed address'}`);
}

for (const r of relays) {
	await release(r.transport, r.alloc, r.creds);
	r.transport.close();
}
line('');
line('   allocations released');
process.exit(0);

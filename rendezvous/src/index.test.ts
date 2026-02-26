import { env, SELF } from 'cloudflare:test';
import { describe, it, expect, beforeEach } from 'vitest';
import { getPublicKeyAsync, signAsync, utils } from '@noble/ed25519';
import { etc } from '@noble/ed25519';

// Configure SHA-512 for @noble/ed25519 v2+ (same as the worker)
etc.sha512Async = async (...messages: Uint8Array[]) => {
	const combined = etc.concatBytes(...messages);
	const digest = await crypto.subtle.digest('SHA-512', combined);
	return new Uint8Array(digest);
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

function bytesToHex(bytes: Uint8Array): string {
	return Array.from(bytes)
		.map((b) => b.toString(16).padStart(2, '0'))
		.join('');
}

async function makeKeypair() {
	const privKey = utils.randomPrivateKey();
	const pubKey = await getPublicKeyAsync(privKey);
	return { privKey, pubKey, pubKeyHex: bytesToHex(pubKey) };
}

async function signMessage(privKey: Uint8Array, message: string): Promise<string> {
	const msgBytes = new TextEncoder().encode(message);
	const sig = await signAsync(msgBytes, privKey);
	return bytesToHex(sig);
}

async function registerSupplier(
	name: string,
	address: string,
	storefrontKey: string,
	keypair: { privKey: Uint8Array; pubKeyHex: string },
) {
	const normalizedName = name.toLowerCase();
	const message = `${normalizedName}|${address}|${storefrontKey}`;
	const signature = await signMessage(keypair.privKey, message);

	return SELF.fetch('http://localhost/register', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({
			name,
			address,
			storefront_key: storefrontKey,
			public_key: keypair.pubKeyHex,
			signature,
		}),
	});
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe('Registration (POST /register)', () => {
	it('1: register with valid signature returns 200', async () => {
		const kp = await makeKeypair();
		const res = await registerSupplier('alice', 'ws://alice:3001', 'sf_key_alice', kp);
		expect(res.status).toBe(200);
		expect(await res.json()).toEqual({ ok: true });
	});

	it('2: lookup after register returns correct record', async () => {
		const kp = await makeKeypair();
		await registerSupplier('bob', 'ws://bob:3001', 'sf_key_bob', kp);

		const res = await SELF.fetch('http://localhost/lookup/bob');
		expect(res.status).toBe(200);
		const body = await res.json() as Record<string, unknown>;
		expect(body).toEqual({
			name: 'bob',
			address: 'ws://bob:3001',
			storefront_key: 'sf_key_bob',
		});
		expect(body).not.toHaveProperty('public_key');
	});

	it('3: re-register same name + same key updates address (idempotent)', async () => {
		const kp = await makeKeypair();
		await registerSupplier('carol', 'ws://carol:3001', 'sf_key_carol', kp);
		await registerSupplier('carol', 'ws://carol:4001', 'sf_key_carol_v2', kp);

		const res = await SELF.fetch('http://localhost/lookup/carol');
		const body = await res.json() as Record<string, unknown>;
		expect(body.address).toBe('ws://carol:4001');
		expect(body.storefront_key).toBe('sf_key_carol_v2');
	});

	it('4: register same name + different key returns 409', async () => {
		const kp1 = await makeKeypair();
		const kp2 = await makeKeypair();
		await registerSupplier('dave', 'ws://dave:3001', 'sf_key_dave', kp1);

		const res = await registerSupplier('dave', 'ws://dave:4001', 'sf_key_dave2', kp2);
		expect(res.status).toBe(409);
		const body = await res.json() as Record<string, unknown>;
		expect(body.error).toBe('Name taken by a different key');
	});

	it('5: invalid signature returns 400', async () => {
		const kp = await makeKeypair();
		// Sign a different message than what the server expects
		const wrongSig = await signMessage(kp.privKey, 'wrong message');

		const res = await SELF.fetch('http://localhost/register', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				name: 'eve',
				address: 'ws://eve:3001',
				storefront_key: 'sf_key_eve',
				public_key: kp.pubKeyHex,
				signature: wrongSig,
			}),
		});
		expect(res.status).toBe(400);
		expect((await res.json() as Record<string, unknown>).error).toBe('Invalid signature');
	});

	it('6: missing required fields returns 400', async () => {
		const fields = ['name', 'address', 'storefront_key', 'public_key', 'signature'];
		for (const omit of fields) {
			const full: Record<string, string> = {
				name: 'test',
				address: 'ws://test:3001',
				storefront_key: 'sf_key',
				public_key: 'deadbeef',
				signature: 'cafebabe',
			};
			delete full[omit];

			const res = await SELF.fetch('http://localhost/register', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify(full),
			});
			expect(res.status).toBe(400);
			expect((await res.json() as Record<string, unknown>).error).toBe('Missing required fields');
		}
	});

	it('7: invalid JSON body returns 400', async () => {
		const res = await SELF.fetch('http://localhost/register', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: 'not json at all{{{',
		});
		expect(res.status).toBe(400);
		expect((await res.json() as Record<string, unknown>).error).toBe('Invalid JSON');
	});

	it('8: name normalization - mixed case resolves to same record', async () => {
		const kp = await makeKeypair();
		await registerSupplier('FrAnK', 'ws://frank:3001', 'sf_key_frank', kp);

		for (const variant of ['frank', 'FRANK', 'Frank', 'fRaNk']) {
			const res = await SELF.fetch(`http://localhost/lookup/${variant}`);
			expect(res.status).toBe(200);
			const body = await res.json() as Record<string, unknown>;
			expect(body.name).toBe('frank');
		}
	});
});

describe('Lookup (GET /lookup/:name)', () => {
	it('9: lookup existing supplier returns 200 with correct fields', async () => {
		const kp = await makeKeypair();
		await registerSupplier('grace', 'ws://grace:3001', 'sf_key_grace', kp);

		const res = await SELF.fetch('http://localhost/lookup/grace');
		expect(res.status).toBe(200);
		expect(await res.json()).toEqual({
			name: 'grace',
			address: 'ws://grace:3001',
			storefront_key: 'sf_key_grace',
		});
	});

	it('10: lookup non-existent name returns 404', async () => {
		const res = await SELF.fetch('http://localhost/lookup/nobody');
		expect(res.status).toBe(404);
		expect((await res.json() as Record<string, unknown>).error).toBe('Supplier not found');
	});

	it('11: lookup is case-insensitive', async () => {
		const kp = await makeKeypair();
		await registerSupplier('henry', 'ws://henry:3001', 'sf_key_henry', kp);

		const res = await SELF.fetch('http://localhost/lookup/HENRY');
		expect(res.status).toBe(200);
		expect((await res.json() as Record<string, unknown>).name).toBe('henry');
	});

	it('12: lookup with empty name returns 400', async () => {
		const res = await SELF.fetch('http://localhost/lookup/');
		expect(res.status).toBe(400);
		expect((await res.json() as Record<string, unknown>).error).toBe('Name required');
	});

	it('13: lookup response excludes public_key', async () => {
		const kp = await makeKeypair();
		await registerSupplier('iris', 'ws://iris:3001', 'sf_key_iris', kp);

		const res = await SELF.fetch('http://localhost/lookup/iris');
		const body = await res.json() as Record<string, unknown>;
		expect(body).not.toHaveProperty('public_key');
	});
});

describe('Heartbeat (POST /heartbeat)', () => {
	it('14: heartbeat with valid signature updates address', async () => {
		const kp = await makeKeypair();
		await registerSupplier('jack', 'ws://jack:3001', 'sf_key_jack', kp);

		const newAddress = 'ws://jack:4001';
		const message = `jack|${newAddress}`;
		const signature = await signMessage(kp.privKey, message);

		const res = await SELF.fetch('http://localhost/heartbeat', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				name: 'jack',
				address: newAddress,
				public_key: kp.pubKeyHex,
				signature,
			}),
		});
		expect(res.status).toBe(200);

		const lookup = await SELF.fetch('http://localhost/lookup/jack');
		expect((await lookup.json() as Record<string, unknown>).address).toBe(newAddress);
	});

	it('15: heartbeat for non-existent supplier returns 404', async () => {
		const kp = await makeKeypair();
		const message = 'ghost|ws://ghost:3001';
		const signature = await signMessage(kp.privKey, message);

		const res = await SELF.fetch('http://localhost/heartbeat', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				name: 'ghost',
				address: 'ws://ghost:3001',
				public_key: kp.pubKeyHex,
				signature,
			}),
		});
		expect(res.status).toBe(404);
		expect((await res.json() as Record<string, unknown>).error).toBe('Supplier not found');
	});

	it('16: heartbeat with wrong key returns 403', async () => {
		const kp1 = await makeKeypair();
		const kp2 = await makeKeypair();
		await registerSupplier('kate', 'ws://kate:3001', 'sf_key_kate', kp1);

		const message = 'kate|ws://kate:4001';
		const signature = await signMessage(kp2.privKey, message);

		const res = await SELF.fetch('http://localhost/heartbeat', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				name: 'kate',
				address: 'ws://kate:4001',
				public_key: kp2.pubKeyHex,
				signature,
			}),
		});
		expect(res.status).toBe(403);
		expect((await res.json() as Record<string, unknown>).error).toBe('Key mismatch');
	});

	it('17: heartbeat with invalid signature returns 400', async () => {
		const kp = await makeKeypair();
		await registerSupplier('leo', 'ws://leo:3001', 'sf_key_leo', kp);

		const wrongSig = await signMessage(kp.privKey, 'wrong message');

		const res = await SELF.fetch('http://localhost/heartbeat', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({
				name: 'leo',
				address: 'ws://leo:4001',
				public_key: kp.pubKeyHex,
				signature: wrongSig,
			}),
		});
		expect(res.status).toBe(400);
		expect((await res.json() as Record<string, unknown>).error).toBe('Invalid signature');
	});

	it('18: heartbeat missing required fields returns 400', async () => {
		const fields = ['name', 'address', 'public_key', 'signature'];
		for (const omit of fields) {
			const full: Record<string, string> = {
				name: 'test',
				address: 'ws://test:3001',
				public_key: 'deadbeef',
				signature: 'cafebabe',
			};
			delete full[omit];

			const res = await SELF.fetch('http://localhost/heartbeat', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify(full),
			});
			expect(res.status).toBe(400);
			expect((await res.json() as Record<string, unknown>).error).toBe('Missing required fields');
		}
	});
});

describe('Routing & CORS', () => {
	it('19: OPTIONS returns CORS headers', async () => {
		const res = await SELF.fetch('http://localhost/register', { method: 'OPTIONS' });
		expect(res.status).toBe(200);
		expect(res.headers.get('Access-Control-Allow-Origin')).toBe('*');
		expect(res.headers.get('Access-Control-Allow-Methods')).toContain('POST');
		expect(res.headers.get('Access-Control-Allow-Headers')).toContain('Content-Type');
	});

	it('20: all responses include CORS Allow-Origin header', async () => {
		// 200 response
		const kp = await makeKeypair();
		const res200 = await registerSupplier('corstest', 'ws://cors:3001', 'sf_key', kp);
		expect(res200.headers.get('Access-Control-Allow-Origin')).toBe('*');

		// 404 response
		const res404 = await SELF.fetch('http://localhost/lookup/nonexistent');
		expect(res404.headers.get('Access-Control-Allow-Origin')).toBe('*');

		// 400 response
		const res400 = await SELF.fetch('http://localhost/register', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: 'bad json',
		});
		expect(res400.headers.get('Access-Control-Allow-Origin')).toBe('*');
	});

	it('21: unknown route returns 404', async () => {
		const res = await SELF.fetch('http://localhost/unknown');
		expect(res.status).toBe(404);
	});

	it('22: wrong HTTP method returns 404', async () => {
		const resGetRegister = await SELF.fetch('http://localhost/register');
		expect(resGetRegister.status).toBe(404);

		const resPostLookup = await SELF.fetch('http://localhost/lookup/test', { method: 'POST' });
		expect(resPostLookup.status).toBe(404);
	});
});

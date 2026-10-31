import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  BridgeError,
  assertBridgeCapabilities,
  discoverBridge,
  normalizeBridgeUrl,
} from '../src/lib/bridge.ts';

test('normalizes loopback bridge origins to HTTP and WebSocket endpoints', () => {
  assert.deepEqual(normalizeBridgeUrl('  http://127.0.0.1:8765/  '), {
    baseUrl: 'http://127.0.0.1:8765',
    eventUrl: 'ws://127.0.0.1:8765/api/v1/events',
  });
  assert.deepEqual(normalizeBridgeUrl('http://localhost:9000'), {
    baseUrl: 'http://localhost:9000',
    eventUrl: 'ws://localhost:9000/api/v1/events',
  });
  assert.equal(normalizeBridgeUrl('http://[::1]:8765').eventUrl, 'ws://[::1]:8765/api/v1/events');
});

test('rejects remote, non-http, and decorated URLs before any token is sent', () => {
  for (const value of [
    'http://192.168.1.10:8765',
    'http://bridge.example.com:8765',
    'http://127.0.0.1.evil.example:8765',
    'https://127.0.0.1:8765',
    'ftp://127.0.0.1:8765',
    'http://127.0.0.1:8765/api/v1',
    'http://user:pass@127.0.0.1:8765',
    'http://127.0.0.1:8765?token=secret',
    'http://127.0.0.1:8765#fragment',
    'not a url',
    '',
  ]) {
    assert.throws(
      () => normalizeBridgeUrl(value),
      (error: unknown) => error instanceof BridgeError,
      value,
    );
  }
});

test('uses stable error codes for the connection form', () => {
  assert.throws(
    () => normalizeBridgeUrl(''),
    (error: unknown) => error instanceof BridgeError && error.code === 'URL_REQUIRED',
  );
  assert.throws(
    () => normalizeBridgeUrl('http://192.168.1.10:8765'),
    (error: unknown) => error instanceof BridgeError && error.code === 'URL_NOT_LOOPBACK',
  );
});

const CAPABILITIES = {
  product: 'omp-bridge',
  apiVersion: 'v1',
  bridgeVersion: '0.1.0',
  eventVersion: 1,
  features: ['pairing', 'status', 'events', 'token-revoke'],
};

test('accepts only a compatible bridge identity', () => {
  assert.deepEqual(assertBridgeCapabilities(CAPABILITIES), CAPABILITIES);
  for (const value of [
    { ...CAPABILITIES, product: 'something-else' },
    { ...CAPABILITIES, apiVersion: 'v2' },
    { ...CAPABILITIES, eventVersion: 2 },
    { ...CAPABILITIES, features: ['status'] },
    {},
    null,
  ]) {
    assert.throws(
      () => assertBridgeCapabilities(value),
      (error: unknown) => error instanceof BridgeError && error.code === 'NOT_A_BRIDGE',
    );
  }
});

test('discovery probes loopback only and returns a compatible bridge', async () => {
  const originalFetch = globalThis.fetch;
  const requested: string[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const url = String(input);
    requested.push(url);
    if (url === 'http://127.0.0.1:9999/api/v1/capabilities') {
      return new Response(JSON.stringify(CAPABILITIES), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }
    return new Response('not found', { status: 404 });
  }) as typeof fetch;
  try {
    const found = await discoverBridge({ ports: [9999], timeoutMs: 50 });
    assert.equal(found.baseUrl, 'http://127.0.0.1:9999');
    assert.equal(found.capabilities.product, 'omp-bridge');
    assert.ok(
      requested.every(
        (url) => url.startsWith('http://127.0.0.1:') || url.startsWith('http://localhost:'),
      ),
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('discovery refuses listeners that are not OMP bridges', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    new Response(JSON.stringify({ product: 'unrelated' }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })) as typeof fetch;
  try {
    await assert.rejects(
      discoverBridge({ ports: [9998], timeoutMs: 50 }),
      (error: unknown) => error instanceof BridgeError && error.code === 'BRIDGE_NOT_FOUND',
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

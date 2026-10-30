import assert from 'node:assert/strict';
import { test } from 'node:test';
import { BridgeError, normalizeBridgeUrl } from '../src/lib/bridge.ts';

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

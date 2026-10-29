#!/usr/bin/env node
// Validates PACK_MANIFEST.json against the checkout and rejects stale prompt-pack paths.
import { readFileSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = normalize(join(dirname(fileURLToPath(import.meta.url)), '..'));
const failures = [];

const manifest = JSON.parse(readFileSync(join(root, 'PACK_MANIFEST.json'), 'utf8'));
if (!Array.isArray(manifest.files) || manifest.files.length === 0) {
  failures.push('PACK_MANIFEST.json must contain a non-empty "files" array');
}

const seen = new Set();
for (const entry of manifest.files ?? []) {
  if (typeof entry !== 'string' || entry.length === 0) {
    failures.push(`invalid manifest entry: ${JSON.stringify(entry)}`);
    continue;
  }
  if (seen.has(entry)) {
    failures.push(`duplicate manifest entry: ${entry}`);
  }
  seen.add(entry);
  if (entry === 'prompts' || entry.startsWith('prompts/')) {
    failures.push(`stale prompt-pack entry: ${entry}`);
  }
  if (isAbsolute(entry) || normalize(entry).startsWith('..')) {
    failures.push(`manifest entry must stay inside the repository: ${entry}`);
  }
  try {
    if (!statSync(join(root, entry)).isFile()) {
      failures.push(`manifest entry is not a file: ${entry}`);
    }
  } catch {
    failures.push(`manifest entry does not exist: ${entry}`);
  }
}

const readme = readFileSync(join(root, 'README.md'), 'utf8');
if (/prompts\//i.test(readme)) {
  failures.push('README.md still references the removed prompts/ directory');
}

if (failures.length > 0) {
  console.error(failures.join('\n'));
  process.exit(1);
}

console.log(`PACK_MANIFEST.json OK: ${manifest.files.length} files present, no prompt-pack references.`);

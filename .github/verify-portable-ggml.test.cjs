const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { mkdtempSync, mkdirSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join } = require('node:path');
const { test } = require('node:test');

const guard = join(__dirname, 'verify-portable-ggml.cjs');
const portableCache = `GGML_NATIVE:BOOL=OFF
GGML_AVX:BOOL=ON
GGML_AVX2:BOOL=ON
GGML_AVX512:BOOL=OFF
GGML_AVX512_VBMI:BOOL=OFF
GGML_AVX512_VNNI:BOOL=OFF
GGML_AVX512_BF16:BOOL=OFF
`;

function runGuard(t, caches, buildPath = 'target/release', args = []) {
  const root = mkdtempSync(join(tmpdir(), 'portable-ggml-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [path, content] of Object.entries(caches)) {
    const directory = join(root, path, 'out/build');
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, 'CMakeCache.txt'), content);
  }
  return spawnSync(process.execPath, [guard, buildPath, ...args], { cwd: root, encoding: 'utf8' });
}

test('requires and reports both native dependencies with AVX2 enabled', (t) => {
  const result = runGuard(t, {
    'target/release/build/whisper-rs-sys-abc': portableCache,
    'target/release/build/llama-cpp-sys-2-def': portableCache,
  }, 'target/release', ['--require-llama']);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Checking .*whisper-rs-sys-/);
  assert.match(result.stdout, /Checking .*llama-cpp-sys-2-/);
  assert.match(result.stdout, /GGML_AVX2:BOOL=ON/);
});

test('rejects missing llama evidence in the selected target/profile', (t) => {
  const result = runGuard(t, {
    'target/x86_64-unknown-linux-gnu/debug/build/whisper-rs-sys-abc': portableCache,
    'target/release/build/llama-cpp-sys-2-def': portableCache,
  }, 'target/x86_64-unknown-linux-gnu/debug', ['--require-llama']);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No llama-helper CMake cache/);
});

for (const dependency of ['whisper-rs-sys', 'llama-cpp-sys-2']) {
  for (const flag of ['GGML_NATIVE', 'GGML_AVX512', 'GGML_AVX512_VBMI', 'GGML_AVX512_VNNI', 'GGML_AVX512_BF16']) {
    test(`rejects unsafe ${flag} in ${dependency}`, (t) => {
      const caches = {
        'target/release/build/whisper-rs-sys-abc': portableCache,
        'target/release/build/llama-cpp-sys-2-abc': portableCache,
      };
      caches[`target/release/build/${dependency}-abc`] = portableCache.replace(`${flag}:BOOL=OFF`, `${flag}:BOOL=ON`);
      const result = runGuard(t, caches, 'target/release', ['--require-llama']);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, new RegExp(`${flag} must be explicitly OFF`));
    });
  }

  for (const compilerFlag of ['-march=native', '-mcpu=native', '-mtune=native']) {
    test(`rejects ${compilerFlag} in ${dependency}`, (t) => {
      const caches = {
        'target/release/build/whisper-rs-sys-abc': portableCache,
        'target/release/build/llama-cpp-sys-2-abc': portableCache,
      };
      caches[`target/release/build/${dependency}-abc`] += `CMAKE_CXX_FLAGS:STRING=-O2 ${compilerFlag}\n`;
      const result = runGuard(t, caches, 'target/release', ['--require-llama']);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /Host-specific compiler flag/);
    });
  }
}

test('accepts portable Whisper configuration with AVX2 enabled', (t) => {
  const result = runGuard(t, { 'target/release/build/whisper-rs-sys-abc': portableCache });
  assert.equal(result.status, 0, result.stderr);
});

test('checks an explicitly selected target and debug profile', (t) => {
  const result = runGuard(t, {
    'target/x86_64-pc-windows-msvc/debug/build/whisper-rs-sys-abc': portableCache,
    'target/release/build/whisper-rs-sys-old': portableCache.replace('GGML_NATIVE:BOOL=OFF', 'GGML_NATIVE:BOOL=ON'),
  }, 'target/x86_64-pc-windows-msvc/debug');
  assert.equal(result.status, 0, result.stderr);
});

test('rejects missing Whisper evidence even if another dependency is portable', (t) => {
  const result = runGuard(t, { 'target/release/build/llama-cpp-sys-abc': portableCache });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No Whisper CMake cache/);
});

for (const flag of ['GGML_NATIVE', 'GGML_AVX512', 'GGML_AVX512_VBMI', 'GGML_AVX512_VNNI', 'GGML_AVX512_BF16']) {
  test(`rejects ${flag} enabled`, (t) => {
    const result = runGuard(t, { 'target/release/build/whisper-rs-sys-abc': portableCache.replace(`${flag}:BOOL=OFF`, `${flag}:BOOL=ON`) });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(`${flag} must be explicitly OFF`));
  });

  test(`rejects missing ${flag} evidence`, (t) => {
    const result = runGuard(t, { 'target/release/build/whisper-rs-sys-abc': portableCache.replace(`${flag}:BOOL=OFF\n`, '') });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(`${flag} must be explicitly OFF`));
  });
}

test('does not let one safe cache hide an unsafe cache in the selected build', (t) => {
  const result = runGuard(t, {
    'target/release/build/whisper-rs-sys-abc': portableCache,
    'target/release/build/whisper-rs-sys-def': portableCache.replace('GGML_NATIVE:BOOL=OFF', 'GGML_NATIVE:BOOL=ON'),
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /GGML_NATIVE must be explicitly OFF/);
});

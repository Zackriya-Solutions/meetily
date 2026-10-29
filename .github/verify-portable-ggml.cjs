const { existsSync, readdirSync, readFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

// Run before bundling so unverified native code cannot become a release asset.
try {
  const requireLlama = process.argv[3] === '--require-llama';
  if (process.argv.length !== (requireLlama ? 4 : 3)) {
    throw new Error('Usage: node verify-portable-ggml.cjs <target/profile directory> [--require-llama]');
  }
  const buildDirectory = resolve(process.argv[2], 'build');
  const entries = existsSync(buildDirectory)
    ? readdirSync(buildDirectory, { withFileTypes: true })
    : [];
  const dependencies = [['whisper-rs-sys-', 'Whisper']];
  if (requireLlama) dependencies.push(['llama-cpp-sys-2-', 'llama-helper']);
  const caches = dependencies.flatMap(([prefix, label]) => {
    const matches = entries
      .filter((entry) => entry.isDirectory() && entry.name.startsWith(prefix))
      .map((entry) => join(buildDirectory, entry.name, 'out/build/CMakeCache.txt'))
      .filter(existsSync);
    if (matches.length === 0) {
      throw new Error(`No ${label} CMake cache found in ${buildDirectory}`);
    }
    return matches;
  });

  for (const cache of caches) {
    const contents = readFileSync(cache, 'utf8');
    console.log(`Checking ${cache}`);
    for (const line of contents.split(/\r?\n/)) {
      if (/^(GGML_(NATIVE|AVX[^:]*|SSE[^:]*|FMA|F16C|BMI[^:]*|AMX[^:]*)|CMAKE_C(?:XX)?_FLAGS[^:]*):/.test(line)) {
        console.log(line);
      }
      if (/^CMAKE_C(?:XX)?_FLAGS[^:]*:/.test(line)
          && /(?:^|\s|["'])-(?:march|mcpu|mtune)=native(?=$|\s|["'])/.test(line.slice(line.indexOf('=') + 1))) {
        throw new Error(`Host-specific compiler flag in ${cache}: ${line}`);
      }
    }
    for (const flag of ['GGML_NATIVE', 'GGML_AVX512', 'GGML_AVX512_VBMI', 'GGML_AVX512_VNNI', 'GGML_AVX512_BF16']) {
      if (!new RegExp(`^${flag}:BOOL=OFF\\r?$`, 'm').test(contents)) {
        throw new Error(`${flag} must be explicitly OFF in ${cache}`);
      }
    }
  }
  console.log(`Verified ${requireLlama ? 'Whisper and llama-helper' : 'Whisper'} CPU portability before bundling.`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

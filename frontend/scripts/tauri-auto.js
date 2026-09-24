#!/usr/bin/env node
/**
 * Auto-detect GPU and run Tauri with appropriate features
 */

const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

// Get the command (dev or build) and optional feature
const command = process.argv[2];
if (!command || !['dev', 'build'].includes(command)) {
  console.error('Usage: node tauri-auto.js [dev|build] [feature]');
  process.exit(1);
}

// Detect GPU feature
let feature = process.argv[3] || '';

// Check for environment variable override first
if (process.env.TAURI_GPU_FEATURE) {
  feature = process.env.TAURI_GPU_FEATURE;
  console.log(`🔧 Using forced GPU feature from environment: ${feature}`);
} else if (feature) {
  console.log(`🔧 Using forced GPU feature from argument: ${feature}`);
} else {
  try {
    const result = execSync('node scripts/auto-detect-gpu.js', {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'inherit']
    });
    feature = result.trim();
  } catch (err) {
    // If detection fails, continue with no features
  }
}

console.log(''); // Empty line for spacing

// Platform-specific environment variables
const platform = os.platform();
const env = { ...process.env };

if (platform === 'linux' && feature === 'cuda') {
  console.log('🐧 Linux/CUDA detected: Setting CMAKE flags for NVIDIA GPU');
  env.CMAKE_CUDA_ARCHITECTURES = '75';
  env.CMAKE_CUDA_STANDARD = '17';
  env.CMAKE_POSITION_INDEPENDENT_CODE = 'ON';
}

if (platform === 'win32') {
  // Ensure Cargo, LLVM, Node, and local node_modules/.bin are on PATH
  const extraPaths = [];
  const cargoBin = path.join(os.homedir(), '.cargo', 'bin');
  if (fs.existsSync(cargoBin)) extraPaths.push(cargoBin);
  const nodeDir = path.dirname(process.execPath);
  if (nodeDir && fs.existsSync(nodeDir)) extraPaths.push(nodeDir);
  const npmGlobal = path.join(os.homedir(), 'AppData', 'Roaming', 'npm');
  if (fs.existsSync(npmGlobal)) extraPaths.push(npmGlobal);
  const localBin = path.join(__dirname, '..', 'node_modules', '.bin');
  if (fs.existsSync(localBin)) extraPaths.push(localBin);
  const llvmBin = 'C:\\Program Files\\LLVM\\bin';
  if (fs.existsSync(llvmBin)) {
    extraPaths.push(llvmBin);
    if (!env.LIBCLANG_PATH) {
      env.LIBCLANG_PATH = llvmBin;
    }
  }
  const cudaPath = process.env.CUDA_PATH || 'C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA\\v13.3';
  if (cudaPath) {
    const cudaBin64 = path.join(cudaPath, 'bin', 'x64');
    if (fs.existsSync(cudaBin64) && !extraPaths.includes(cudaBin64)) extraPaths.push(cudaBin64);
    const cudaBin = path.join(cudaPath, 'bin');
    if (fs.existsSync(cudaBin) && !extraPaths.includes(cudaBin)) extraPaths.push(cudaBin);
  }

  // Ensure CMake and Ninja are on PATH for whisper-rs-sys and CUDA builds
  const cmakePaths = [
    'C:\\Program Files\\CMake\\bin',
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
    'C:\\Program Files\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
    'C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
    'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin',
  ];
  for (const cp of cmakePaths) {
    if (fs.existsSync(cp) && !extraPaths.includes(cp)) {
      extraPaths.push(cp);
      break;
    }
  }

  const ninjaPaths = [
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
    'C:\\Program Files\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
    'C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
    'C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
    'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\Ninja',
  ];
  for (const np of ninjaPaths) {
    if (fs.existsSync(np) && !extraPaths.includes(np)) {
      extraPaths.push(np);
      break;
    }
  }

  if (extraPaths.length > 0) {
    env.PATH = `${extraPaths.join(';')};${env.PATH || ''}`;
  }

  if (feature === 'cuda') {
    console.log('🪟 Windows/CUDA detected: Setting CMAKE flags for NVIDIA GPU');
    env.CMAKE_CUDA_ARCHITECTURES = env.CMAKE_CUDA_ARCHITECTURES || '75';
    env.CMAKE_CUDA_STANDARD = env.CMAKE_CUDA_STANDARD || '17';
    env._CL_ = env._CL_ ? `${env._CL_} /Zc:preprocessor` : '/Zc:preprocessor';
  }
}

// Build the tauri command using local binary directly
const localTauri = path.join(__dirname, '..', 'node_modules', '.bin', process.platform === 'win32' ? 'tauri.cmd' : 'tauri');
const tauriBin = fs.existsSync(localTauri) ? `"${localTauri}"` : 'tauri';
let tauriCmd = `${tauriBin} ${command}`;
if (command === 'build') {
  tauriCmd += ' --ignore-version-mismatches';
}
if (feature && feature !== 'none') {
  tauriCmd += ` -- --features ${feature}`;
  console.log(`🚀 Running: tauri ${command} with features: ${feature}`);
} else {
  console.log(`🚀 Running: tauri ${command} (CPU-only mode)`);
}
console.log('');

// Execute the command
try {
  execSync(tauriCmd, { stdio: 'inherit', env });
} catch (err) {
  process.exit(err.status || 1);
}

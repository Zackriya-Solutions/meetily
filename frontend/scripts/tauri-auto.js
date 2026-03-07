#!/usr/bin/env node
/**
 * Auto-detect GPU and run Tauri with appropriate features.
 * Kills any stale process on the Next.js dev port before starting.
 */

const { execSync, spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

// Kill anything already bound to port 3118 so a stale Next.js server
// from a previous run never blocks the new one.
const DEV_PORT = 3118;
try {
  const platform = os.platform();
  if (platform === 'win32') {
    // netstat + taskkill on Windows
    const result = spawnSync('cmd', ['/c', `for /f "tokens=5" %a in ('netstat -aon ^| findstr :${DEV_PORT}') do taskkill /F /PID %a`], { encoding: 'utf8' });
    if (result.stdout && result.stdout.includes('SUCCESS')) {
      console.log(`🧹 Killed stale process on port ${DEV_PORT}`);
    }
  } else {
    // lsof on macOS/Linux
    const pids = spawnSync('lsof', ['-ti', `tcp:${DEV_PORT}`], { encoding: 'utf8' }).stdout.trim();
    if (pids) {
      pids.split('\n').filter(Boolean).forEach(pid => {
        try {
          process.kill(parseInt(pid, 10), 'SIGKILL');
          console.log(`🧹 Killed stale process ${pid} on port ${DEV_PORT}`);
        } catch (_) {}
      });
    }
  }
} catch (_) {
  // Non-fatal — if nothing is running the kill is a no-op
}

// Get the command (dev or build)
const command = process.argv[2];
if (!command || !['dev', 'build'].includes(command)) {
  console.error('Usage: node tauri-auto.js [dev|build]');
  process.exit(1);
}

// Detect GPU feature
let feature = '';

// Check for environment variable override first
if (process.env.TAURI_GPU_FEATURE) {
  feature = process.env.TAURI_GPU_FEATURE;
  console.log(`🔧 Using forced GPU feature from environment: ${feature}`);
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

// Build the tauri command
let tauriCmd = `tauri ${command}`;
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

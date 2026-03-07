/**
 * No-op localStorage / sessionStorage shim for SSR.
 * Injected into the server-side webpack bundle via ProvidePlugin so that
 * 'use client' code that calls localStorage.getItem / setItem etc. during
 * the Next.js dev-server render pass doesn't throw.
 */

const noopStorage = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
  clear: () => {},
  key: () => null,
  length: 0,
};

module.exports = noopStorage;



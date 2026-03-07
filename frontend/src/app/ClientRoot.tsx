'use client';

import dynamic from 'next/dynamic';

// Dynamically import the full app shell with ssr:false.
// This prevents any browser-only APIs (localStorage, Tauri, etc.)
// from being called during the Next.js server-side render pass.
const AppShell = dynamic(() => import('./AppShell'), { ssr: false });

export default function ClientRoot({ children }: { children: React.ReactNode }) {
  return <AppShell>{children}</AppShell>;
}


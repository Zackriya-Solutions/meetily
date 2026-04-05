'use client';

import { useEffect } from 'react';
import { AlertTriangle } from 'lucide-react';

import { Button } from '@/components/ui/button';

export default function GlobalError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error('[app/global-error] Root layout error', error);
  }, [error]);

  return (
    <html lang="en">
      <body className="bg-slate-950 text-slate-50">
        <div className="flex min-h-screen items-center justify-center px-6">
          <div className="w-full max-w-xl rounded-2xl border border-slate-800 bg-slate-900 p-8 shadow-2xl">
            <div className="mb-4 flex items-center gap-3">
              <AlertTriangle className="h-6 w-6 text-amber-400" />
              <h1 className="text-2xl font-semibold">Application error</h1>
            </div>
            <p className="mb-6 text-sm text-slate-300">
              The app failed at the root layout level. Retry once. If it fails again,
              restart the app to fully recover.
            </p>
            {error.digest ? (
              <p className="mb-6 text-xs text-slate-400">Error digest: {error.digest}</p>
            ) : null}
            <div className="flex gap-3">
              <Button onClick={() => reset()}>Try Again</Button>
              <Button variant="outline" onClick={() => window.location.reload()}>
                Reload App
              </Button>
            </div>
          </div>
        </div>
      </body>
    </html>
  );
}

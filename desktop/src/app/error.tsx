'use client';

import { useEffect } from 'react';
import { AlertCircle } from 'lucide-react';

import { Button } from '@/components/ui/button';

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error('[app/error] Unhandled route error', error);
  }, [error]);

  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-50 px-6">
      <div className="w-full max-w-lg rounded-2xl border border-slate-200 bg-white p-8 shadow-sm">
        <div className="mb-4 flex items-center gap-3 text-slate-900">
          <AlertCircle className="h-6 w-6 text-amber-600" />
          <h1 className="text-2xl font-semibold">Something went wrong</h1>
        </div>
        <p className="mb-6 text-sm text-slate-600">
          The current screen failed to render. Try the action again. If the problem
          persists, restart the app and inspect the logs.
        </p>
        {error.digest ? (
          <p className="mb-6 text-xs text-slate-500">Error digest: {error.digest}</p>
        ) : null}
        <div className="flex gap-3">
          <Button onClick={() => reset()}>Try Again</Button>
          <Button variant="outline" onClick={() => window.location.assign('/')}>
            Go Home
          </Button>
        </div>
      </div>
    </div>
  );
}

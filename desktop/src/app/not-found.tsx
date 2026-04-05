import Link from 'next/link';

import { Button } from '@/components/ui/button';

export default function NotFound() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-50 px-6">
      <div className="w-full max-w-lg rounded-2xl border border-slate-200 bg-white p-8 shadow-sm">
        <p className="mb-2 text-sm font-medium uppercase tracking-[0.2em] text-slate-500">
          404
        </p>
        <h1 className="mb-3 text-3xl font-semibold text-slate-900">Page not found</h1>
        <p className="mb-6 text-sm text-slate-600">
          The requested route does not exist in this desktop app.
        </p>
        <Button asChild>
          <Link href="/">Return Home</Link>
        </Button>
      </div>
    </div>
  );
}

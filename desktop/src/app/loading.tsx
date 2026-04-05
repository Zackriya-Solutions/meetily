export default function Loading() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-slate-50 px-6">
      <div className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-8 shadow-sm">
        <div className="mb-4 h-2 w-24 animate-pulse rounded-full bg-slate-200" />
        <div className="mb-3 h-6 w-48 animate-pulse rounded bg-slate-200" />
        <div className="mb-2 h-4 w-full animate-pulse rounded bg-slate-100" />
        <div className="mb-2 h-4 w-5/6 animate-pulse rounded bg-slate-100" />
        <div className="h-4 w-3/4 animate-pulse rounded bg-slate-100" />
      </div>
    </div>
  );
}

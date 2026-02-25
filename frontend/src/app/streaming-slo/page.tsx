'use client';

import React, { useEffect, useMemo, useState } from 'react';
import { authFetch } from '@/lib/api';
import { Loader2, RefreshCcw, Activity, AlertTriangle } from 'lucide-react';
import { toast } from 'sonner';

interface StreamingSloSession {
  session_id: string;
  meeting_id: string;
  user_email: string;
  status: string;
  started_at: string | null;
  alerts_count: number;
  alert_counts: Record<string, number>;
  streaming_slo: {
    first_stable_emit_latency_seconds?: number | null;
    avg_segment_finalize_latency_seconds?: number | null;
    stable_segments?: number;
    volatile_segments?: number;
    correction_rate?: number;
    drift_rate?: number;
    health?: {
      latency_degraded?: boolean;
      stability_degraded?: boolean;
      transport_degraded?: boolean;
    };
  };
}

interface StreamingSloResponse {
  scope: {
    lookback_hours: number;
    started_after: string;
    user_filter: string;
    session_limit: number;
  };
  summary: {
    sessions_with_slo: number;
    latency_degraded_sessions: number;
    stability_degraded_sessions: number;
    transport_degraded_sessions: number;
    avg_first_stable_emit_latency_seconds?: number | null;
    avg_segment_finalize_latency_seconds?: number | null;
    slo_target_seconds: number;
    slo_max_seconds: number;
  };
  sessions: StreamingSloSession[];
}

const LOOKBACK_OPTIONS = [6, 12, 24, 48, 72];

const fmt = (value?: number | null, digits = 2): string => {
  if (value === undefined || value === null) return '-';
  return value.toFixed(digits);
};

export default function StreamingSloPage() {
  const [lookbackHours, setLookbackHours] = useState(24);
  const [loading, setLoading] = useState(true);
  const [report, setReport] = useState<StreamingSloResponse | null>(null);

  const fetchReport = async () => {
    setLoading(true);
    try {
      const res = await authFetch(`/streaming/slo-report?lookback_hours=${lookbackHours}&limit=500`);
      if (!res.ok) {
        if (res.status === 403) {
          throw new Error('Access denied for streaming SLO report');
        }
        throw new Error(`Failed to load report (${res.status})`);
      }
      const data = (await res.json()) as StreamingSloResponse;
      setReport(data);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load report';
      toast.error(message);
      setReport(null);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void fetchReport();
  }, [lookbackHours]);

  const sessions = useMemo(() => report?.sessions ?? [], [report]);

  return (
    <div className="flex flex-col h-screen bg-gray-50 overflow-hidden">
      <div className="flex-1 overflow-y-auto p-8">
        <div className="max-w-6xl mx-auto space-y-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h1 className="text-2xl font-bold text-gray-900 flex items-center gap-2">
                <Activity className="w-6 h-6 text-blue-600" />
                Streaming SLO Report
              </h1>
              <p className="text-sm text-gray-600 mt-1">
                Reliability health for real-time transcription sessions.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <select
                value={lookbackHours}
                onChange={(e) => setLookbackHours(Number(e.target.value))}
                className="h-9 px-3 border border-gray-300 rounded-md bg-white text-sm"
              >
                {LOOKBACK_OPTIONS.map((h) => (
                  <option key={h} value={h}>
                    Last {h}h
                  </option>
                ))}
              </select>
              <button
                onClick={() => void fetchReport()}
                className="h-9 px-3 rounded-md border border-gray-300 bg-white hover:bg-gray-100 text-sm flex items-center gap-2"
              >
                <RefreshCcw className="w-4 h-4" />
                Refresh
              </button>
            </div>
          </div>

          {loading ? (
            <div className="flex justify-center py-20">
              <Loader2 className="w-8 h-8 animate-spin text-blue-500" />
            </div>
          ) : !report ? (
            <div className="bg-white border border-gray-200 rounded-xl p-6 text-sm text-gray-600">
              No report available.
            </div>
          ) : (
            <>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div className="bg-white border border-gray-200 rounded-xl p-4">
                  <div className="text-xs text-gray-500">Sessions With SLO</div>
                  <div className="text-2xl font-semibold text-gray-900 mt-1">
                    {report.summary.sessions_with_slo}
                  </div>
                  <div className="text-xs text-gray-600 mt-2">
                    Avg first stable: {fmt(report.summary.avg_first_stable_emit_latency_seconds)}s
                  </div>
                  <div className="text-xs text-gray-600">
                    Avg finalize: {fmt(report.summary.avg_segment_finalize_latency_seconds)}s
                  </div>
                </div>
                <div className="bg-white border border-gray-200 rounded-xl p-4">
                  <div className="text-xs text-gray-500">Degraded Sessions</div>
                  <div className="flex gap-3 mt-2 text-sm">
                    <span className="px-2 py-1 rounded bg-amber-100 text-amber-800">
                      Latency: {report.summary.latency_degraded_sessions}
                    </span>
                    <span className="px-2 py-1 rounded bg-orange-100 text-orange-800">
                      Stability: {report.summary.stability_degraded_sessions}
                    </span>
                    <span className="px-2 py-1 rounded bg-red-100 text-red-800">
                      Transport: {report.summary.transport_degraded_sessions}
                    </span>
                  </div>
                </div>
                <div className="bg-white border border-gray-200 rounded-xl p-4">
                  <div className="text-xs text-gray-500">SLO Targets</div>
                  <div className="text-sm text-gray-700 mt-2">
                    Target: <strong>{fmt(report.summary.slo_target_seconds, 1)}s</strong>
                  </div>
                  <div className="text-sm text-gray-700">
                    Max: <strong>{fmt(report.summary.slo_max_seconds, 1)}s</strong>
                  </div>
                  <div className="text-xs text-gray-500 mt-2">
                    Scope: {report.scope.user_filter} · {report.scope.lookback_hours}h
                  </div>
                </div>
              </div>

              <div className="bg-white border border-gray-200 rounded-xl overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-200 text-sm font-medium text-gray-700">
                  Session Details ({sessions.length})
                </div>
                <div className="overflow-x-auto">
                  <table className="min-w-full text-xs">
                    <thead className="bg-gray-50 text-gray-600">
                      <tr>
                        <th className="text-left px-4 py-2">Session</th>
                        <th className="text-left px-4 py-2">User</th>
                        <th className="text-left px-4 py-2">Latency</th>
                        <th className="text-left px-4 py-2">Stability</th>
                        <th className="text-left px-4 py-2">Alerts</th>
                        <th className="text-left px-4 py-2">Health</th>
                      </tr>
                    </thead>
                    <tbody>
                      {sessions.map((item) => {
                        const health = item.streaming_slo.health || {};
                        return (
                          <tr key={item.session_id} className="border-t border-gray-100">
                            <td className="px-4 py-2">
                              <div className="font-medium text-gray-800">{item.session_id.slice(0, 8)}...</div>
                              <div className="text-gray-500">{item.started_at ? new Date(item.started_at).toLocaleString() : '-'}</div>
                            </td>
                            <td className="px-4 py-2 text-gray-700">{item.user_email}</td>
                            <td className="px-4 py-2 text-gray-700">
                              first: {fmt(item.streaming_slo.first_stable_emit_latency_seconds)}s
                              <br />
                              avg: {fmt(item.streaming_slo.avg_segment_finalize_latency_seconds)}s
                            </td>
                            <td className="px-4 py-2 text-gray-700">
                              corr: {fmt(item.streaming_slo.correction_rate, 3)}
                              <br />
                              drift: {fmt(item.streaming_slo.drift_rate, 3)}
                            </td>
                            <td className="px-4 py-2 text-gray-700">
                              <div className="font-medium">{item.alerts_count}</div>
                              <div className="text-gray-500 truncate max-w-[200px]">
                                {Object.entries(item.alert_counts || {})
                                  .map(([k, v]) => `${k}:${v}`)
                                  .join(', ') || '-'}
                              </div>
                            </td>
                            <td className="px-4 py-2">
                              <div className="flex flex-wrap gap-1">
                                {health.latency_degraded && (
                                  <span className="px-2 py-0.5 rounded bg-amber-100 text-amber-800 flex items-center gap-1">
                                    <AlertTriangle className="w-3 h-3" />
                                    latency
                                  </span>
                                )}
                                {health.stability_degraded && (
                                  <span className="px-2 py-0.5 rounded bg-orange-100 text-orange-800 flex items-center gap-1">
                                    <AlertTriangle className="w-3 h-3" />
                                    stability
                                  </span>
                                )}
                                {health.transport_degraded && (
                                  <span className="px-2 py-0.5 rounded bg-red-100 text-red-800 flex items-center gap-1">
                                    <AlertTriangle className="w-3 h-3" />
                                    transport
                                  </span>
                                )}
                                {!health.latency_degraded && !health.stability_degraded && !health.transport_degraded && (
                                  <span className="px-2 py-0.5 rounded bg-green-100 text-green-800">
                                    healthy
                                  </span>
                                )}
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

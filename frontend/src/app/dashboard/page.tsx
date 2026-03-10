'use client';

import React, { useEffect, useState } from 'react';
import { useSession } from 'next-auth/react';
import { useRouter } from 'next/navigation';
import { authFetch } from '@/lib/api';
import { 
  BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip as RechartsTooltip, ResponsiveContainer, 
  PieChart, Pie, Cell, LineChart, Line, Legend
} from 'recharts';
import { 
  Activity, Users, Mic, Layers, ArrowLeft, Loader2 
} from 'lucide-react';

export default function DashboardPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [loading, setLoading] = useState(true);
  const [metrics, setMetrics] = useState<any>(null);
  const [userFilter, setUserFilter] = useState<string>('exclude_admin'); // 'all', 'exclude_admin', or specific email

  useEffect(() => {
    // Check auth and admin
    if (status === 'unauthenticated') {
      router.push('/login');
      return;
    }

    if (status === 'authenticated') {
      if (session?.user?.email !== 'gagan@appointy.com') {
        router.push('/'); // Redirect non-admins to home
        return;
      }
      fetchMetrics();
    }
  }, [status, session, router, userFilter]);

  const fetchMetrics = async () => {
    try {
      setLoading(true);
      const response = await authFetch(`/analytics/dashboard/metrics?user_filter=${encodeURIComponent(userFilter)}`);
      
      if (!response.ok) {
        throw new Error('Failed to fetch metrics: ' + response.statusText);
      }
      
      const data = await response.json();
      
      if (data && data.kpis && data.kpis.totalEvents !== undefined) {
        setMetrics(data);
      } else {
        console.error('Invalid analytics data format received:', data);
        setMetrics({
          kpis: { totalEvents: 0, uniqueUsers: 0 },
          featureBreakdown: [],
          templatePopularity: [],
          dailyUsage: []
        });
      }
    } catch (error) {
      console.error('Failed to fetch metrics:', error);
      // Fallback state so the UI renders gracefully
      setMetrics({
        kpis: { totalEvents: 0, uniqueUsers: 0 },
        featureBreakdown: [],
        templatePopularity: [],
        dailyUsage: []
      });
    } finally {
      setLoading(false);
    }
  };

  if (status === 'loading' || loading) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50">
        <div className="flex flex-col items-center">
          <Loader2 className="h-8 w-8 animate-spin text-blue-600 mb-4" />
          <p className="text-gray-500">Loading Pnyx Analytics...</p>
        </div>
      </div>
    );
  }

  if (!metrics) {
    return (
      <div className="p-8">
        <h1 className="text-2xl font-bold mb-4">Analytics Dashboard</h1>
        <p className="text-red-500">Failed to load analytics data. Ensure the backend is running.</p>
      </div>
    );
  }

  const COLORS = ['#0088FE', '#00C49F', '#FFBB28', '#FF8042', '#8884d8', '#82ca9d', '#ffc658'];

  return (
    <div className="h-full bg-gray-50 flex flex-col">
      {/* Fixed Header */}
      <div className="sticky top-0 z-10 bg-gray-50 border-b border-gray-200">
        <div className="max-w-7xl mx-auto px-8 py-6">
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-3xl font-bold text-gray-900 tracking-tight">Pnyx Analytics</h1>
              <p className="text-gray-500 mt-1">Admin Dashboard - Feature Usage & Outcomes</p>
            </div>
            <button 
              onClick={() => router.push('/')}
              className="flex items-center px-4 py-2 bg-white border border-gray-200 rounded-lg text-sm font-medium text-gray-700 hover:bg-gray-50 transition-colors"
            >
              <ArrowLeft className="w-4 h-4 mr-2" />
              Back to App
            </button>
          </div>
        </div>
      </div>

      {/* Scrollable Content */}
      <div className="flex-1 overflow-y-auto pb-24">
        <div className="max-w-7xl mx-auto p-8 pt-6 space-y-8">
          {/* Controls Row */}
          <div className="flex justify-end">
            <div className="flex items-center gap-3">
              <label className="text-sm font-medium text-gray-700">Filter Users:</label>
              <select 
                value={userFilter}
                onChange={(e) => setUserFilter(e.target.value)}
                className="block w-48 pl-3 pr-10 py-2 text-sm border border-gray-300 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md"
              >
                <option value="exclude_admin">Real Users (Exclude Admin & Anonymous)</option>
                <option value="all">All Events (Including Admin)</option>
                <option disabled>──────────</option>
                {metrics?.uniqueUsersList?.map((u: string) => (
                  <option key={u} value={u}>{u}</option>
                ))}
              </select>
            </div>
          </div>

          {/* Top KPIs */}
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex flex-col">
            <div className="flex items-center justify-between text-blue-600 mb-4">
              <Activity className="w-6 h-6" />
              <span className="text-xs font-semibold uppercase tracking-wider bg-blue-50 px-2 py-1 rounded-full">Total Events</span>
            </div>
            <div className="text-4xl font-bold text-gray-900">{metrics.kpis.totalEvents}</div>
          </div>
          
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex flex-col">
            <div className="flex items-center justify-between text-green-600 mb-4">
              <Users className="w-6 h-6" />
              <span className="text-xs font-semibold uppercase tracking-wider bg-green-50 px-2 py-1 rounded-full">Unique Users</span>
            </div>
            <div className="text-4xl font-bold text-gray-900">{metrics.kpis.uniqueUsers}</div>
          </div>

          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex flex-col">
            <div className="flex items-center justify-between text-purple-600 mb-4">
              <Mic className="w-6 h-6" />
              <span className="text-xs font-semibold uppercase tracking-wider bg-purple-50 px-2 py-1 rounded-full">Meetings Started</span>
            </div>
            <div className="text-4xl font-bold text-gray-900">
              {metrics.featureBreakdown.find((f: any) => f.name === 'meeting_started')?.value || 0}
            </div>
          </div>

          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex flex-col">
            <div className="flex items-center justify-between text-orange-600 mb-4">
              <Layers className="w-6 h-6" />
              <span className="text-xs font-semibold uppercase tracking-wider bg-orange-50 px-2 py-1 rounded-full">Notes Shared</span>
            </div>
            <div className="text-4xl font-bold text-gray-900">
              {metrics.featureBreakdown.find((f: any) => f.name === 'notes_shared')?.value || 0}
            </div>
          </div>
        </div>

        {/* Charts Row 1 */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
          {/* Daily Active Usage (Line Chart) */}
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6">
            <h2 className="text-lg font-semibold text-gray-800 mb-6">Engagement Over Time (7 Days)</h2>
            <div className="h-72">
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={metrics.dailyUsage}>
                  <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#E5E7EB" />
                  <XAxis dataKey="date" axisLine={false} tickLine={false} tick={{fill: '#6B7280', fontSize: 12}} dy={10} />
                  <YAxis axisLine={false} tickLine={false} tick={{fill: '#6B7280', fontSize: 12}} dx={-10} />
                  <RechartsTooltip 
                    contentStyle={{borderRadius: '8px', border: 'none', boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1)'}}
                  />
                  <Line type="monotone" dataKey="events" name="Total Events" stroke="#3B82F6" strokeWidth={3} dot={{r: 4, strokeWidth: 2}} activeDot={{r: 6}} />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          {/* Feature Usage Breakdown (Bar Chart) */}
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6">
            <h2 className="text-lg font-semibold text-gray-800 mb-6">Top Pnyx Features Used</h2>
            <div className="h-72">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={metrics.featureBreakdown.slice(0, 8)} layout="vertical" margin={{ left: 40 }}>
                  <CartesianGrid strokeDasharray="3 3" horizontal={false} stroke="#E5E7EB" />
                  <XAxis type="number" axisLine={false} tickLine={false} tick={{fill: '#6B7280', fontSize: 12}} />
                  <YAxis dataKey="name" type="category" axisLine={false} tickLine={false} tick={{fill: '#4B5563', fontSize: 11}} width={120} />
                  <RechartsTooltip 
                    cursor={{fill: '#F3F4F6'}}
                    contentStyle={{borderRadius: '8px', border: 'none', boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1)'}}
                  />
                  <Bar dataKey="value" name="Usage Count" fill="#10B981" radius={[0, 4, 4, 0]}>
                    {metrics.featureBreakdown.map((entry: any, index: number) => (
                      <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>
        </div>

        {/* Charts Row 2 */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
          {/* Notes Templates (Pie Chart) */}
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 lg:col-span-1">
            <h2 className="text-lg font-semibold text-gray-800 mb-6">Popular Note Templates</h2>
            {metrics.templatePopularity.length > 0 ? (
              <div className="h-64 relative">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={metrics.templatePopularity}
                      cx="50%"
                      cy="50%"
                      innerRadius={60}
                      outerRadius={80}
                      paddingAngle={5}
                      dataKey="value"
                    >
                      {metrics.templatePopularity.map((entry: any, index: number) => (
                        <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
                      ))}
                    </Pie>
                    <RechartsTooltip 
                      contentStyle={{borderRadius: '8px', border: 'none', boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1)'}}
                    />
                  </PieChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="h-64 flex items-center justify-center text-sm text-gray-400 border-2 border-dashed border-gray-100 rounded-lg">
                No template data yet
              </div>
            )}
            {/* Custom Legend for Pie */}
            <div className="mt-4 flex flex-wrap gap-2 justify-center">
              {metrics.templatePopularity.map((entry: any, index: number) => (
                <div key={`legend-${index}`} className="flex items-center text-xs text-gray-600">
                  <span className="w-3 h-3 rounded-full mr-1.5" style={{backgroundColor: COLORS[index % COLORS.length]}}></span>
                  {entry.name} ({entry.value})
                </div>
              ))}
            </div>
          </div>

          {/* AI Value Generation (List) */}
          <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 lg:col-span-2">
            <h2 className="text-lg font-semibold text-gray-800 mb-6">AI Co-Pilot Engagement</h2>
            
            <div className="space-y-4">
              <EngagementRow 
                title="Diarization (Speaker Separation)" 
                count={metrics.featureBreakdown.find((f: any) => f.name === 'diarization_requested')?.value || 0}
                color="bg-blue-500"
                total={metrics.kpis.totalEvents}
              />
              <EngagementRow 
                title="Catch Up Used" 
                count={metrics.featureBreakdown.find((f: any) => f.name === 'catch_up_requested')?.value || 0}
                color="bg-green-500"
                total={metrics.kpis.totalEvents}
              />
              <EngagementRow 
                title="Ask AI Queries" 
                count={metrics.featureBreakdown.find((f: any) => f.name === 'ask_ai_query')?.value || 0}
                color="bg-purple-500"
                total={metrics.kpis.totalEvents}
              />
              <EngagementRow 
                title="Notes Refined by AI" 
                count={metrics.featureBreakdown.find((f: any) => f.name === 'notes_refined')?.value || 0}
                color="bg-orange-500"
                total={metrics.kpis.totalEvents}
              />
              <EngagementRow 
                title="Cross-Meeting Context Linked" 
                count={metrics.featureBreakdown.find((f: any) => f.name === 'cross_meeting_context_linked')?.value || 0}
                color="bg-indigo-500"
                total={metrics.kpis.totalEvents}
              />
            </div>
          </div>
        </div>

      </div>
    </div>
    </div>
  );
}

// Helper component for the Engagement list
function EngagementRow({ title, count, color, total }: { title: string, count: number, color: string, total: number }) {
  const percentage = total > 0 ? Math.min(100, Math.round((count / total) * 100 * 5)) : 0; // *5 just to make bars visible for demo with low total counts

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between items-center text-sm font-medium text-gray-700">
        <span>{title}</span>
        <span className="bg-gray-100 text-gray-600 px-2 py-0.5 rounded text-xs">{count} uses</span>
      </div>
      <div className="w-full bg-gray-100 rounded-full h-2">
        <div className={`${color} h-2 rounded-full`} style={{ width: `${percentage}%` }}></div>
      </div>
    </div>
  );
}

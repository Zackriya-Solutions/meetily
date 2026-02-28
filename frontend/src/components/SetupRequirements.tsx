'use client';

import { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import { authFetch } from '@/lib/api';
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from '@/components/ui/button';
import { Key, Calendar, ArrowRight } from 'lucide-react';

export function SetupRequirements() {
  const router = useRouter();
  const [missingKeys, setMissingKeys] = useState<string[]>([]);
  const [calendarConnected, setCalendarConnected] = useState<boolean>(true); // Default to true to avoid flash
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const checkRequirements = async () => {
      try {
        const [keysRes, calendarRes] = await Promise.all([
          authFetch('/api/user/keys'),
          authFetch('/api/calendar/status')
        ]);

        if (keysRes.ok) {
          const keys = await keysRes.json();
          const missing = [];
          if (!keys.groq) missing.push('Groq');
          if (!keys.deepgram) missing.push('Deepgram');
          setMissingKeys(missing);
        }

        if (calendarRes.ok) {
          const status = await calendarRes.json();
          setCalendarConnected(status.connected);
        }
      } catch (error) {
        console.error('Failed to check setup requirements:', error);
      } finally {
        setLoading(false);
      }
    };

    checkRequirements();
  }, []);

  if (loading) return null;

  const showKeysAlert = missingKeys.length > 0;
  const showCalendarAlert = !calendarConnected;

  if (!showKeysAlert && !showCalendarAlert) return null;

  return (
    <div className="space-y-3 mb-6">
      {showKeysAlert && (
        <Alert className="bg-amber-50 border-amber-200">
          <Key className="h-4 w-4 text-amber-600" />
          <AlertTitle className="text-amber-800">API Keys Required</AlertTitle>
          <AlertDescription className="text-amber-700 flex items-center justify-between mt-1">
            <span>
              Please configure your {missingKeys.join(' and ')} API keys to enable transcription and diarization features.
            </span>
            <Button 
              size="sm" 
              variant="outline" 
              className="ml-4 bg-white border-amber-300 hover:bg-amber-100 text-amber-900"
              onClick={() => router.push('/settings?tab=personalKeys')}
            >
              Configure Keys <ArrowRight className="ml-1 h-3 w-3" />
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {showCalendarAlert && (
        <Alert className="bg-blue-50 border-blue-200">
          <Calendar className="h-4 w-4 text-blue-600" />
          <AlertTitle className="text-blue-800">Connect Calendar</AlertTitle>
          <AlertDescription className="text-blue-700 flex items-center justify-between mt-1">
            <span>
              Connect your Google Calendar to automatically join meetings and sync summaries.
            </span>
            <Button 
              size="sm" 
              variant="outline" 
              className="ml-4 bg-white border-blue-300 hover:bg-blue-100 text-blue-900"
              onClick={() => router.push('/settings?tab=calendar')}
            >
              Connect Calendar <ArrowRight className="ml-1 h-3 w-3" />
            </Button>
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}

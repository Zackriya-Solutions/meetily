'use client';

import { useState, useEffect } from 'react';
import { useSession } from 'next-auth/react';
import { authFetch } from '@/lib/api';
import { Calendar, ArrowRight, X } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

export function CalendarConnectPrompt() {
  const { data: session, status } = useSession();
  const [isOpen, setIsOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (status !== 'authenticated' || !session?.user?.email) {
      return;
    }

    const checkAndSyncCalendar = async () => {
      try {
        const promptKey = `calendarPromptSeen_${session.user.email}`;
        
        // Check current status in backend
        const res = await authFetch('/api/calendar/status');
        if (res.ok) {
          const data = await res.json();
          
          if (!data.connected) {
            // Check if they granted scopes via NextAuth login
            const sess = session as any;
            if (sess.calendarScopes && sess.accessToken) {
              console.log('User granted calendar scopes at login. Syncing with backend...');
              try {
                // Sync to backend
                const syncRes = await authFetch('/api/calendar/sync-oauth', {
                  method: 'POST',
                  body: JSON.stringify({
                    access_token: sess.accessToken,
                    refresh_token: sess.refreshToken,
                    token_expires_at: null, // Let backend calculate or we ignore
                    scopes: sess.rawScopes ? sess.rawScopes.split(' ') : [],
                    external_account_email: session.user.email
                  })
                });
                
                if (syncRes.ok) {
                  console.log('Calendar synced successfully!');
                  setIsLoading(false);
                  return; // Don't show modal
                }
              } catch (e) {
                console.error('Failed to sync calendar tokens:', e);
              }
            }
            
            // If they didn't grant scopes or sync failed, show modal (once per session)
            if (!sessionStorage.getItem(promptKey)) {
              setIsOpen(true);
              sessionStorage.setItem(promptKey, 'true');
            }
          }
        }
      } catch (err) {
        console.error('Failed to check calendar status', err);
      } finally {
        setIsLoading(false);
      }
    };

    checkAndSyncCalendar();
  }, [status, session]);

  const handleConnect = async () => {
    try {
      const response = await authFetch('/api/calendar/connect?request_write_scope=false', {
        method: 'POST',
      });
      if (!response.ok) throw new Error('Failed to get auth URL');
      const data = await response.json();
      if (data.auth_url) {
        window.location.href = data.auth_url;
      }
    } catch (error) {
      console.error('Failed to connect calendar:', error);
    }
  };

  if (!isOpen) return null;

  return (
    <Dialog open={isOpen} onOpenChange={setIsOpen}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <div className="mx-auto w-12 h-12 bg-blue-100 rounded-full flex items-center justify-center mb-4">
            <Calendar className="w-6 h-6 text-blue-600" />
          </div>
          <DialogTitle className="text-center text-xl">Connect Your Calendar</DialogTitle>
          <DialogDescription className="text-center pt-2">
            Connect your Google Calendar to automatically receive meeting notes and action items for your upcoming meetings.
          </DialogDescription>
        </DialogHeader>
        
        <div className="flex flex-col gap-3 mt-4">
          <Button onClick={handleConnect} className="w-full bg-blue-600 hover:bg-blue-700 text-white">
            Connect Google Calendar <ArrowRight className="w-4 h-4 ml-2" />
          </Button>
          <Button onClick={() => setIsOpen(false)} variant="ghost" className="w-full text-gray-500">
            Maybe Later
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

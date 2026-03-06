'use client';

import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { authFetch } from '@/lib/api';
import { Share2, Clock, User, ChevronRight } from 'lucide-react';

export default function SharedNotesPage() {
  const router = useRouter();
  const [sharedNotes, setSharedNotes] = useState<any[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    async function fetchShared() {
      try {
        const res = await authFetch('/api/sharing/shared-with-me');
        if (res.ok) {
          const data = await res.json();
          setSharedNotes(data);
        }
      } catch (err) {
        console.error('Failed to fetch shared notes', err);
      } finally {
        setIsLoading(false);
      }
    }
    fetchShared();
  }, []);

  const handleNoteClick = (meetingId: string) => {
    router.push(`/meeting-details?id=${meetingId}&shared=true`);
  };

  return (
    <div className="max-w-4xl mx-auto p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold flex items-center text-gray-800">
          <Share2 className="w-6 h-6 mr-3 text-blue-500" />
          Shared with Me
        </h1>
        <p className="text-gray-500 mt-2">
          Meeting notes and transcripts shared with you by other organizers.
        </p>
      </div>

      {isLoading ? (
        <div className="flex justify-center py-12">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
        </div>
      ) : sharedNotes.length === 0 ? (
        <div className="text-center py-16 bg-gray-50 rounded-lg border border-gray-100">
          <Share2 className="w-12 h-12 text-gray-300 mx-auto mb-4" />
          <h3 className="text-lg font-medium text-gray-700">No shared notes yet</h3>
          <p className="text-gray-500 mt-2">When someone shares meeting notes with you, they'll appear here.</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {sharedNotes.map((note) => {
            const isUnread = !note.last_viewed_at ||
              (note.notes_updated_at && new Date(note.notes_updated_at) > new Date(note.last_viewed_at));

            return (
              <div
                key={note.id}
                onClick={() => handleNoteClick(note.meeting_id)}
                className={`flex items-center p-5 rounded-xl border transition-all cursor-pointer hover:shadow-md
                  ${isUnread ? 'bg-white border-blue-200 shadow-sm relative' : 'bg-gray-50 border-gray-200 hover:bg-white'}`}
              >
                {isUnread && (
                  <div className="absolute top-0 left-0 w-1 h-full bg-blue-500 rounded-l-xl"></div>
                )}

                <div className="flex-1 min-w-0 pr-4 pl-2">
                  <div className="flex items-center gap-3 mb-1">
                    <h2 className={`text-lg truncate ${isUnread ? 'font-bold text-gray-900' : 'font-medium text-gray-700'}`}>
                      {note.meeting_title || 'Untitled Meeting'}
                    </h2>
                    {isUnread && (
                      <span className="px-2 py-0.5 bg-blue-100 text-blue-700 text-xs font-semibold rounded-full">
                        {note.notes_updated_at && note.last_viewed_at ? 'Updated' : 'New'}
                      </span>
                    )}
                  </div>

                  <div className="flex flex-wrap items-center gap-4 text-sm text-gray-500">
                    <div className="flex items-center">
                      <User className="w-4 h-4 mr-1.5" />
                      <span className="truncate max-w-[200px]">{note.owner_email}</span>
                    </div>
                    <div className="flex items-center">
                      <Clock className="w-4 h-4 mr-1.5" />
                      {new Date(note.shared_at).toLocaleDateString(undefined, {
                        month: 'short', day: 'numeric', year: 'numeric'
                      })}
                    </div>
                  </div>
                </div>

                <div className="flex items-center justify-center w-10 h-10 rounded-full bg-gray-100 group-hover:bg-blue-50 transition-colors">
                  <ChevronRight className={`w-5 h-5 ${isUnread ? 'text-blue-500' : 'text-gray-400'}`} />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

import React, { useState, useEffect } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { format, parseISO } from 'date-fns';
import { Calendar as CalendarIcon, Clock, Users, Loader2 } from 'lucide-react';
import { authFetch } from '@/lib/api';

export interface CalendarEvent {
    event_id: string;
    meeting_title: string;
    meeting_link?: string;
    agenda_description?: string;
    attendees: Array<{ email: string; name?: string }>;
    start_time: string;
    end_time: string;
}

interface CalendarMeetingPickerProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSelectMeeting: (event: CalendarEvent) => void;
}

export function CalendarMeetingPicker({ open, onOpenChange, onSelectMeeting }: CalendarMeetingPickerProps) {
    const [events, setEvents] = useState<CalendarEvent[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (open) {
            loadUpcomingMeetings();
        }
    }, [open]);

    const loadUpcomingMeetings = async () => {
        setLoading(true);
        setError(null);
        try {
            const response = await authFetch('/api/calendar/upcoming-meetings');
            if (!response.ok) {
                throw new Error('Failed to fetch upcoming meetings');
            }
            const data = await response.json();
            if (data.status === 'success') {
                setEvents(data.events || []);
            } else {
                throw new Error(data.message || 'Error loading meetings');
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Unknown error');
        } finally {
            setLoading(false);
        }
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[500px]">
                <DialogHeader>
                    <DialogTitle className="flex items-center gap-2">
                        <CalendarIcon className="h-5 w-5 text-indigo-600" />
                        Pick Calendar Meeting
                    </DialogTitle>
                    <DialogDescription>
                        Select an upcoming meeting to auto-fill the title and provide context for the AI Copilot.
                    </DialogDescription>
                </DialogHeader>

                <div className="py-4 space-y-4 max-h-[60vh] overflow-y-auto">
                    {loading ? (
                        <div className="flex flex-col items-center justify-center py-8 text-gray-500">
                            <Loader2 className="h-8 w-8 animate-spin mb-4 text-indigo-500" />
                            <p>Loading upcoming meetings...</p>
                        </div>
                    ) : error ? (
                        <div className="text-center py-4 px-3 bg-red-50 text-red-600 rounded-md text-sm border border-red-100">
                            {error}
                        </div>
                    ) : events.length === 0 ? (
                        <div className="text-center py-8 text-gray-500">
                            <CalendarIcon className="h-10 w-10 mx-auto text-gray-300 mb-3" />
                            <p>No upcoming meetings found in the next 12 hours.</p>
                            <p className="text-sm mt-1">Make sure your Google Calendar is connected.</p>
                        </div>
                    ) : (
                        <div className="space-y-3">
                            {events.map((event) => (
                                <div
                                    key={event.event_id}
                                    onClick={() => {
                                        onSelectMeeting(event);
                                        onOpenChange(false);
                                    }}
                                    className="flex flex-col p-4 rounded-xl border border-gray-200 hover:border-indigo-300 hover:shadow-md hover:bg-slate-50 transition-all cursor-pointer group"
                                >
                                    <h3 className="font-medium text-gray-900 mb-1 group-hover:text-indigo-700 transition-colors">
                                        {event.meeting_title || 'Untitled Meeting'}
                                    </h3>

                                    <div className="flex items-center text-xs text-slate-500 mt-2 gap-4">
                                        <span className="flex items-center gap-1.5 font-medium">
                                            <Clock className="w-3.5 h-3.5" />
                                            {format(parseISO(event.start_time), 'h:mm a')} - {format(parseISO(event.end_time), 'h:mm a')}
                                        </span>
                                        {(event.attendees?.length > 0) && (
                                            <span className="flex items-center gap-1.5">
                                                <Users className="w-3.5 h-3.5" />
                                                {event.attendees.length} participant{event.attendees.length !== 1 ? 's' : ''}
                                            </span>
                                        )}
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>
                        Cancel
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}

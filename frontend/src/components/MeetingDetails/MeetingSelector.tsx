import React, { useState, useEffect } from 'react';
import { Search, Link as LinkIcon, Check, Loader2, Calendar } from 'lucide-react';
import { Meeting } from '@/types';
import { authFetch } from '@/lib/api';

interface MeetingSelectorProps {
    selectedIds: string[];
    onSelectionChange: (ids: string[]) => void;
}

export function MeetingSelector({ selectedIds, onSelectionChange }: MeetingSelectorProps) {
    const [meetings, setMeetings] = useState<Meeting[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [searchQuery, setSearchQuery] = useState('');

    useEffect(() => {
        const fetchMeetings = async () => {
            try {
                const response = await authFetch('/list-meetings');
                if (response.ok) {
                    const data = await response.json();
                    setMeetings(data);
                }
            } catch (error) {
                console.error("Failed to fetch meetings:", error);
            } finally {
                setIsLoading(false);
            }
        };

        fetchMeetings();
    }, []);

    const filteredMeetings = meetings.filter(m =>
        (m.title || m.id).toLowerCase().includes(searchQuery.toLowerCase())
    );

    const toggleSelection = (id: string) => {
        if (selectedIds.includes(id)) {
            onSelectionChange(selectedIds.filter(sid => sid !== id));
        } else {
            onSelectionChange([...selectedIds, id]);
        }
    };

    return (
        <div className="flex flex-col h-full max-h-[400px]">
            {/* Search Header */}
            <div className="p-3 border-b border-zinc-200 dark:border-zinc-800">
                <div className="relative">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400" />
                    <input
                        type="text"
                        placeholder="Search past meetings..."
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        className="w-full pl-9 pr-3 py-1.5 text-sm bg-zinc-100 dark:bg-zinc-800 border-none rounded-md focus:ring-2 focus:ring-blue-500 outline-none text-zinc-900 dark:text-zinc-100"
                    />
                </div>
            </div>

            {/* List */}
            <div className="flex-1 overflow-y-auto p-2 space-y-1">
                {isLoading ? (
                    <div className="flex justify-center p-4">
                        <Loader2 className="w-5 h-5 animate-spin text-zinc-400" />
                    </div>
                ) : filteredMeetings.length === 0 ? (
                    <div className="text-center text-sm text-zinc-500 p-4">
                        No meetings found
                    </div>
                ) : (
                    filteredMeetings.map(meeting => {
                        const isSelected = selectedIds.includes(meeting.id);
                        return (
                            <button
                                key={meeting.id}
                                onClick={() => toggleSelection(meeting.id)}
                                className={`
                                    w-full text-left px-3 py-2 rounded-lg flex items-start gap-3 transition-colors
                                    ${isSelected
                                        ? 'bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800'
                                        : 'hover:bg-zinc-100 dark:hover:bg-zinc-800 border border-transparent'}
                                `}
                            >
                                <div className={`mt-0.5 w-4 h-4 rounded border flex items-center justify-center shrink-0 
                                    ${isSelected
                                        ? 'bg-blue-500 border-blue-500 text-white'
                                        : 'border-zinc-300 dark:border-zinc-600'}
                                `}>
                                    {isSelected && <Check className="w-3 h-3" />}
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className={`text-sm font-medium truncate ${isSelected ? 'text-blue-700 dark:text-blue-300' : 'text-zinc-900 dark:text-zinc-100'}`}>
                                        {meeting.title || 'Untitled Meeting'}
                                    </div>
                                    <div className="flex items-center gap-2 text-xs text-zinc-500 mt-0.5">
                                        <Calendar className="w-3 h-3" />
                                        <span>{new Date(meeting.date).toLocaleDateString()}</span>
                                        <span>•</span>
                                        <span>{new Date(meeting.date).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
                                    </div>
                                </div>
                            </button>
                        );
                    })
                )}
            </div>

            <div className="p-3 border-t border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50 text-xs text-zinc-500 flex justify-between items-center">
                <span>{selectedIds.length} linked</span>
                {selectedIds.length > 0 && (
                    <button
                        onClick={() => onSelectionChange([])}
                        className="text-blue-500 hover:text-blue-600 font-medium"
                    >
                        Clear All
                    </button>
                )}
            </div>
        </div>
    );
}

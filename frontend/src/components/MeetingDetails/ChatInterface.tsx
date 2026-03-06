import React, { useState, useRef, useEffect, useCallback } from 'react';
import { Send, Bot, User, Loader2, X, Link as LinkIcon, ChevronDown, ChevronUp, GripVertical } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { Transcript } from '@/types';
import { MeetingSelector } from './MeetingSelector';
import { authFetch } from '@/lib/api';

interface Message {
    role: 'user' | 'assistant';
    content: string;
}

interface ChatInterfaceProps {
    meetingId: string;
    onClose: () => void;
    currentTranscripts?: Transcript[];
}

// Simple markdown renderer component
function MarkdownContent({ content }: { content: string }) {
    const renderMarkdown = (text: string) => {
        const lines = text.split('\n');
        const elements: React.ReactNode[] = [];
        let listItems: string[] = [];
        let listType: 'ul' | 'ol' | null = null;

        const flushList = () => {
            if (listItems.length > 0 && listType) {
                const ListTag = listType;
                elements.push(
                    <ListTag key={elements.length} className={listType === 'ul' ? 'list-disc ml-4 my-2 space-y-1' : 'list-decimal ml-4 my-2 space-y-1'}>
                        {listItems.map((item, i) => <li key={i} className="text-sm">{renderInline(item)}</li>)}
                    </ListTag>
                );
                listItems = [];
                listType = null;
            }
        };

        const renderInline = (line: string): React.ReactNode => {
            // Bold: **text** or __text__
            line = line.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
            line = line.replace(/__(.+?)__/g, '<strong>$1</strong>');
            // Italic: *text* or _text_
            line = line.replace(/\*([^*]+)\*/g, '<em>$1</em>');
            // Code: `code`
            line = line.replace(/`([^`]+)`/g, '<code class="bg-zinc-200 dark:bg-zinc-700 px-1 rounded text-xs">$1</code>');
            // Links: [text](url)
            line = line.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" class="text-blue-500 underline">$1</a>');

            return <span dangerouslySetInnerHTML={{ __html: line }} />;
        };

        lines.forEach((line, index) => {
            // Headers
            if (line.startsWith('### ')) {
                flushList();
                elements.push(<h3 key={index} className="font-bold text-base mt-3 mb-1 text-zinc-900 dark:text-zinc-100">{renderInline(line.slice(4))}</h3>);
            } else if (line.startsWith('## ')) {
                flushList();
                elements.push(<h2 key={index} className="font-bold text-lg mt-4 mb-2 text-zinc-900 dark:text-zinc-100">{renderInline(line.slice(3))}</h2>);
            } else if (line.startsWith('# ')) {
                flushList();
                elements.push(<h1 key={index} className="font-bold text-xl mt-4 mb-2 text-zinc-900 dark:text-zinc-100">{renderInline(line.slice(2))}</h1>);
            }
            // Bullet lists
            else if (line.match(/^[\*\-]\s+/)) {
                if (listType !== 'ul') flushList();
                listType = 'ul';
                listItems.push(line.replace(/^[\*\-]\s+/, ''));
            }
            // Numbered lists
            else if (line.match(/^\d+\.\s+/)) {
                if (listType !== 'ol') flushList();
                listType = 'ol';
                listItems.push(line.replace(/^\d+\.\s+/, ''));
            }
            // Horizontal rule
            else if (line.match(/^---+$/)) {
                flushList();
                elements.push(<hr key={index} className="my-3 border-zinc-300 dark:border-zinc-700" />);
            }
            // Regular paragraph
            else if (line.trim()) {
                flushList();
                elements.push(<p key={index} className="text-sm my-1">{renderInline(line)}</p>);
            }
            // Empty line
            else {
                flushList();
                elements.push(<div key={index} className="h-2" />);
            }
        });

        flushList();
        return elements;
    };

    return <div className="markdown-content">{renderMarkdown(content)}</div>;
}

export function ChatInterface({ meetingId, onClose, currentTranscripts }: ChatInterfaceProps) {
    const [messages, setMessages] = useState<Message[]>([]);
    const [input, setInput] = useState('');
    const [isLoading, setIsLoading] = useState(false);
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const { serverAddress } = useSidebar();

    // Resizable panel state
    const [panelWidth, setPanelWidth] = useState(450);
    const isResizing = useRef(false);
    const containerRef = useRef<HTMLDivElement>(null);

    // Scoped Search State
    const [showLinkContext, setShowLinkContext] = useState(false);
    const [linkedMeetingIds, setLinkedMeetingIds] = useState<string[]>([]);

    // Handle resize
    const handleMouseDown = useCallback((e: React.MouseEvent) => {
        isResizing.current = true;
        e.preventDefault();
    }, []);

    useEffect(() => {
        const handleMouseMove = (e: MouseEvent) => {
            if (!isResizing.current) return;
            const newWidth = window.innerWidth - e.clientX;
            setPanelWidth(Math.max(350, Math.min(800, newWidth)));
        };

        const handleMouseUp = () => {
            isResizing.current = false;
        };

        document.addEventListener('mousemove', handleMouseMove);
        document.addEventListener('mouseup', handleMouseUp);

        return () => {
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
        };
    }, []);

    const getStableContextEntries = () => {
        if (!currentTranscripts || currentTranscripts.length === 0) return [];
        return currentTranscripts
            .filter((t) => (t.stability_class || 'stable') === 'stable')
            .slice(-400)
            .map((t) => ({
                text: t.text,
                timestamp: t.timestamp,
                audio_start_time: t.audio_start_time,
                audio_end_time: t.audio_end_time,
                stability_score: t.stability_score,
                stability_class: t.stability_class || 'stable',
                is_stable: (t.stability_class || 'stable') !== 'volatile'
            }));
    };

    // Construct fallback plain context from live transcripts if available
    const getContextFromTranscripts = () => {
        const entries = getStableContextEntries();
        if (entries.length === 0) return undefined;
        return entries.map((t) => `[${t.timestamp}] ${t.text}`).join('\n');
    };

    const scrollToBottom = () => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    };

    useEffect(() => {
        scrollToBottom();
    }, [messages]);

    // State for model config
    const [modelConfig, setModelConfig] = useState<{ provider: string, model: string } | null>(null);

    // Fetch model config on mount
    useEffect(() => {
        const fetchConfig = async () => {
            try {
                const res = await authFetch('/get-model-config');
                if (res.ok) {
                    const config = await res.json();
                    setModelConfig(config);
                }
            } catch (e) {
                console.error("Failed to fetch model config", e);
            }
        };
        fetchConfig();
    }, [serverAddress]);


    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!input.trim() || isLoading) return;

        const userMessage = input.trim();
        setInput('');
        setMessages(prev => [...prev, { role: 'user', content: userMessage }]);
        setIsLoading(true);

        try {
            // Create a placeholder for the AI response
            setMessages(prev => [...prev, { role: 'assistant', content: '' }]);

            // Use configured model or factory default
            const provider = modelConfig?.provider || 'gemini';
            const modelName = modelConfig?.model || 'gemini-2.5-pro';

            const contextEntries = getStableContextEntries();
            const contextText = getContextFromTranscripts();

            const response = await authFetch('/chat-meeting', {
                method: 'POST',
                // headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    meeting_id: meetingId,
                    question: userMessage,
                    model: provider,
                    model_name: modelName,
                    context_text: contextText || "",
                    context_entries: contextEntries,
                    allowed_meeting_ids: linkedMeetingIds.length > 0 ? linkedMeetingIds : undefined,
                    history: messages.slice(-10).map(m => ({
                        role: m.role,
                        content: m.content.slice(0, 500)
                    }))
                }),
            });

            if (!response.ok) {
                throw new Error(`Request failed: ${response.status}`);
            }

            if (!response.body) throw new Error('No response body');

            const reader = response.body.getReader();

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;

                const chunk = new TextDecoder().decode(value);
                setMessages(prev => {
                    const newMessages = [...prev];
                    const lastMessage = newMessages[newMessages.length - 1];
                    if (lastMessage.role === 'assistant') {
                        lastMessage.content += chunk;
                    }
                    return newMessages;
                });
            }
        } catch (error) {
            console.error('Chat error:', error);
            setMessages(prev => [...prev, { role: 'assistant', content: 'Sorry, I encountered an error while processing your request.' }]);
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <div
            ref={containerRef}
            className="flex flex-col h-full bg-white dark:bg-zinc-900 border-l border-zinc-200 dark:border-zinc-800 shadow-xl fixed right-0 top-0 bottom-0 z-50"
            style={{ width: `${panelWidth}px` }}
        >
            {/* Resize handle */}
            <div
                className="absolute left-0 top-0 bottom-0 w-2 cursor-ew-resize hover:bg-blue-500/20 flex items-center justify-center group"
                onMouseDown={handleMouseDown}
            >
                <GripVertical className="w-3 h-3 text-zinc-400 group-hover:text-blue-500 opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>

            {/* Header */}
            <div className="flex flex-col border-b border-zinc-200 dark:border-zinc-800">
                <div className="flex items-center justify-between p-4 pb-2">
                    <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
                        <Bot className="w-5 h-5 text-blue-500" />
                        Ask AI
                    </h3>
                    <div className="flex items-center gap-1">
                        <button
                            onClick={() => setShowLinkContext(!showLinkContext)}
                            className={`
                                flex items-center gap-1.5 px-2 py-1 text-xs font-medium rounded-full transition-colors
                                ${linkedMeetingIds.length > 0
                                    ? 'bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300'
                                    : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700'}
                            `}
                        >
                            <LinkIcon className="w-3 h-3" />
                            {linkedMeetingIds.length > 0 ? `${linkedMeetingIds.length} Linked` : 'Link Context'}
                            {showLinkContext ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
                        </button>
                        <button
                            onClick={onClose}
                            className="p-1 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-full transition-colors ml-1"
                        >
                            <X className="w-5 h-5 text-zinc-500" />
                        </button>
                    </div>
                </div>

                {/* Context Linker Dropdown */}
                {showLinkContext && (
                    <div className="border-t border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50">
                        <MeetingSelector
                            selectedIds={linkedMeetingIds}
                            onSelectionChange={setLinkedMeetingIds}
                        />
                    </div>
                )}
            </div>

            {/* Messages */}
            <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {messages.length === 0 && (
                    <div className="text-center text-zinc-500 mt-10 space-y-2">
                        <Bot className="w-12 h-12 mx-auto opacity-20" />
                        <p>Ask anything about this meeting!</p>
                        {linkedMeetingIds.length > 0 && (
                            <p className="text-xs text-blue-500 font-medium">
                                Searching {linkedMeetingIds.length} linked meeting(s)
                            </p>
                        )}
                        <p className="text-xs text-zinc-400">"What were the key decisions?"</p>
                        <p className="text-xs text-zinc-400">"search on web [your query]"</p>
                    </div>
                )}

                {messages.map((msg, idx) => (
                    <div
                        key={idx}
                        className={`flex gap-3 ${msg.role === 'user' ? 'flex-row-reverse' : 'flex-row'}`}
                    >
                        <div className={`
              w-8 h-8 rounded-full flex items-center justify-center shrink-0
              ${msg.role === 'user' ? 'bg-blue-100 text-blue-600' : 'bg-green-100 text-green-600'}
            `}>
                            {msg.role === 'user' ? <User className="w-4 h-4" /> : <Bot className="w-4 h-4" />}
                        </div>
                        <div className={`
              rounded-lg p-3 max-w-[90%]
              ${msg.role === 'user'
                                ? 'bg-blue-500 text-white text-sm'
                                : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200'}
            `}>
                            {msg.role === 'assistant' ? (
                                !msg.content && isLoading && idx === messages.length - 1 ? (
                                    <div className="flex items-center gap-2 text-zinc-500 dark:text-zinc-400">
                                        <Loader2 className="w-4 h-4 animate-spin" />
                                        <span className="text-sm">Thinking...</span>
                                    </div>
                                ) : (
                                    <MarkdownContent content={msg.content} />
                                )
                            ) : (
                                msg.content
                            )}
                        </div>
                    </div>
                ))}
                <div ref={messagesEndRef} />
            </div>

            {/* Input */}
            <form onSubmit={handleSubmit} className="p-4 border-t border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
                <div className="relative">
                    <input
                        type="text"
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        placeholder="Ask a question... (or 'search on web [query]')"
                        className="w-full pl-4 pr-10 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 dark:text-zinc-100"
                        disabled={isLoading}
                    />
                    <button
                        type="submit"
                        disabled={!input.trim() || isLoading}
                        className="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-blue-500 hover:text-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />}
                    </button>
                </div>
            </form>
        </div>
    );
}

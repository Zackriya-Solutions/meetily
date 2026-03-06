import React, { useState, useRef, useEffect } from 'react';
import { Send, Bot, User, Loader2, X, Check, ArrowRight } from 'lucide-react';
import { authFetch } from '@/lib/api';
import { Button } from '@/components/ui/button';

interface Message {
    role: 'user' | 'assistant';
    content: string;
    isRefinement?: boolean; // If true, this message contains a refined version of notes
}

interface RefineNotesSidebarProps {
    meetingId: string;
    onClose: () => void;
    currentNotes: string;
    onApplyRefinement: (newNotes: string) => void;
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

export function RefineNotesSidebar({ meetingId, onClose, currentNotes, onApplyRefinement }: RefineNotesSidebarProps) {
    const [messages, setMessages] = useState<Message[]>([
        { role: 'assistant', content: 'Hi! I can help you refine these notes. You can say things like "Fix typos", "Make the tone more formal", or "Add a section about Next Steps".' }
    ]);
    const [input, setInput] = useState('');
    const [isLoading, setIsLoading] = useState(false);
    const messagesEndRef = useRef<HTMLDivElement>(null);

    const scrollToBottom = () => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    };

    useEffect(() => {
        scrollToBottom();
    }, [messages]);

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!input.trim() || isLoading) return;

        const userMessage = input.trim();
        setInput('');
        setMessages(prev => [...prev, { role: 'user', content: userMessage }]);
        setIsLoading(true);

        try {
            // Placeholder for streaming response
            setMessages(prev => [...prev, { role: 'assistant', content: '', isRefinement: true }]);

            // We need a way to accumulate the stream into the LAST message
            let accumulatedContent = "";

            const response = await authFetch('/refine-notes', {
                method: 'POST',
                body: JSON.stringify({
                    meeting_id: meetingId,
                    current_notes: currentNotes,
                    user_instruction: userMessage,
                    model: 'gemini', // Default to gemini for now
                    model_name: 'gemini-2.5-pro'
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
                accumulatedContent += chunk;

                setMessages(prev => {
                    const newMessages = [...prev];
                    const lastMessage = newMessages[newMessages.length - 1];
                    if (lastMessage.role === 'assistant') {
                        lastMessage.content = accumulatedContent;
                    }
                    return newMessages;
                });
            }

        } catch (error) {
            console.error('Refinement error:', error);
            setMessages(prev => {
                // Remove the empty assistant message if it failed immediately or append error
                const newMessages = [...prev];
                const lastMessage = newMessages[newMessages.length - 1];
                if (lastMessage.role === 'assistant' && lastMessage.content === '') {
                    newMessages.pop(); // Remove empty placeholder
                }
                return [...newMessages, { role: 'assistant', content: 'Sorry, I encountered an error while refining your notes.' }];
            });
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <div className="flex flex-col h-full bg-white border-l border-zinc-200 shadow-xl fixed right-0 top-0 bottom-0 z-50 w-[400px]">
            {/* Header */}
            <div className="flex items-center justify-between p-4 border-b border-zinc-200 bg-gradient-to-r from-purple-50 to-blue-50">
                <h3 className="font-semibold text-zinc-900 flex items-center gap-2">
                    <Bot className="w-5 h-5 text-purple-600" />
                    Refine Notes
                </h3>
                <button
                    onClick={onClose}
                    className="p-1 hover:bg-zinc-200 rounded-full transition-colors"
                >
                    <X className="w-5 h-5 text-zinc-500" />
                </button>
            </div>

            {/* Messages */}
            <div className="flex-1 overflow-y-auto p-4 space-y-4 bg-zinc-50">
                {messages.map((msg, idx) => (
                    <div
                        key={idx}
                        className={`flex flex-col gap-2 ${msg.role === 'user' ? 'items-end' : 'items-start'}`}
                    >
                        <div className={`
                            rounded-lg p-3 max-w-[90%] shadow-sm
                            ${msg.role === 'user'
                                ? 'bg-blue-600 text-white'
                                : 'bg-white text-zinc-800 border border-zinc-200'}
                        `}>
                            {msg.role === 'assistant' && !msg.content && isLoading && idx === messages.length - 1 ? (
                                <div className="flex items-center gap-2 text-zinc-500">
                                    <Loader2 className="w-4 h-4 animate-spin" />
                                    <span className="text-sm">Thinking...</span>
                                </div>
                            ) : (
                                /* Split content logic */
                                <MarkdownContent content={msg.content.split('|||SEPARATOR|||')[0]} />
                            )}
                        </div>

                        {/* Apply Button for Refinements */}
                        {msg.role === 'assistant' && msg.isRefinement && msg.content && !isLoading && idx === messages.length - 1 && (
                            <div className="flex gap-2 mt-1">
                                <Button
                                    size="sm"
                                    variant="default"
                                    className="bg-green-600 hover:bg-green-700 h-8 text-xs"
                                    onClick={() => {
                                        const parts = msg.content.split('|||SEPARATOR|||');
                                        if (parts.length > 1) {
                                            onApplyRefinement(parts[1].trim());
                                        } else {
                                            // Fallback: Apply whole content if no separator found
                                            onApplyRefinement(msg.content.trim());
                                        }
                                    }}
                                >
                                    <Check className="w-3 h-3 mr-1" />
                                    Apply Changes
                                </Button>
                            </div>
                        )}
                    </div>
                ))}
                <div ref={messagesEndRef} />
            </div>

            {/* Input */}
            <form onSubmit={handleSubmit} className="p-4 border-t border-zinc-200 bg-white">
                <div className="relative">
                    <input
                        type="text"
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        placeholder="How should I change the notes?"
                        className="w-full pl-4 pr-10 py-3 rounded-lg border border-zinc-300 bg-transparent focus:outline-none focus:ring-2 focus:ring-purple-500"
                        disabled={isLoading}
                    />
                    <button
                        type="submit"
                        disabled={!input.trim() || isLoading}
                        className="absolute right-2 top-1/2 -translate-y-1/2 p-2 text-purple-600 hover:text-purple-700 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isLoading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />}
                    </button>
                </div>
            </form>
        </div>
    );
}

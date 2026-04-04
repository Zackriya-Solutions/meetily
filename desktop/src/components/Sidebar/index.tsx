'use client';

import React, { useState, useMemo, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, File, Settings, Calendar, Home, Trash2, Mic, Square, Plus, Pencil, NotebookPen, SearchIcon, X, Upload, FileOutput } from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar } from './SidebarProvider';
import type { CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import Analytics from '@/lib/analytics';
import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"

import Logo from '../Logo';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '../ui/input-group';

interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
}

interface BatchExportResult {
  meeting_id: string;
  output_path: string | null;
  success: boolean;
  error: string | null;
}

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const isHomePage = pathname === '/';
  const isMeetingPage = pathname?.includes('/meeting-details');
  const isSettingsPage = pathname === '/settings';
  const {
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    searchFilters,
    setSearchFilters,
    meetings,
    setMeetings
  } = useSidebar();

  // Get recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set(['meetings']));
  const [searchQuery, setSearchQuery] = useState<string>('');
  const hasActiveSearchFilters =
    !!searchFilters.dateFrom ||
    !!searchFilters.dateTo ||
    searchFilters.sourceType !== 'all' ||
    searchFilters.hasSummary !== 'all';

  // State for edit modal
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null; currentTitle: string }>({
    isOpen: false,
    meetingId: null,
    currentTitle: ''
  });
  const [editingTitle, setEditingTitle] = useState<string>('');

  // Ensure 'meetings' folder is always expanded
  useEffect(() => {
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders]);


  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({ isOpen: false, itemId: null });
  const [isBatchExportOpen, setIsBatchExportOpen] = useState(false);
  const [batchDestinationRoot, setBatchDestinationRoot] = useState('');
  const [selectedForBatchExport, setSelectedForBatchExport] = useState<Set<string>>(new Set());
  const [isBatchExporting, setIsBatchExporting] = useState(false);
  const [batchExportResults, setBatchExportResults] = useState<BatchExportResult[] | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');

  // Handle search input changes
  const handleSearchChange = useCallback(async (value: string) => {
    setSearchQuery(value);

    await searchTranscripts(value, searchFilters);

    // Make sure the meetings folder is expanded when searching
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders, searchFilters, searchTranscripts]);

  const handleFilterChange = useCallback(async (patch: Partial<typeof searchFilters>) => {
    const nextFilters = { ...searchFilters, ...patch };
    setSearchFilters(nextFilters);
    await searchTranscripts(searchQuery, nextFilters);
  }, [searchFilters, setSearchFilters, searchTranscripts, searchQuery]);

  // Combine search results with sidebar items
  const filteredSidebarItems = useMemo(() => {
    if (!searchQuery.trim() && !hasActiveSearchFilters) return sidebarItems;

    // If we have search results, highlight matching meetings
    if (searchResults.length > 0) {
      // Get the IDs of meetings that matched in transcripts
      const matchedMeetingIds = new Set(searchResults.map(result => result.id));

      return sidebarItems
        .map<SidebarItem | undefined>(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search results or title match
            const filteredChildren = folder.children.filter(item => {
              // Include if the meeting ID is in our search results
              if (matchedMeetingIds.has(item.id)) return true;

              if (!searchQuery.trim()) {
                return false;
              }

              // Or if the title matches the search query
              return item.title.toLowerCase().includes(searchQuery.toLowerCase());
            });

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return (matchedMeetingIds.has(folder.id) ||
            (!!searchQuery.trim() && folder.title.toLowerCase().includes(searchQuery.toLowerCase())))
            ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    } else {
      if (!searchQuery.trim()) {
        return sidebarItems
          .map<SidebarItem | undefined>(folder => {
            if (folder.type === 'folder') {
              return { ...folder, children: [] };
            }
            return undefined;
          })
          .filter((item): item is SidebarItem => item !== undefined);
      }

      // Fall back to title-only filtering if no transcript results
      return sidebarItems
        .map<SidebarItem | undefined>(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search query
            const filteredChildren = folder.children.filter(item =>
              item.title.toLowerCase().includes(searchQuery.toLowerCase())
            );

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return folder.title.toLowerCase().includes(searchQuery.toLowerCase()) ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    }
  }, [sidebarItems, searchQuery, searchResults, hasActiveSearchFilters]);

  // Check if search returned no results
  const hasNoSearchResults = (searchQuery.trim() || hasActiveSearchFilters) && filteredSidebarItems.every(item => !item.children?.length);
  const batchExportCandidates = useMemo(
    () => meetings.filter((meeting: CurrentMeeting) => meeting.id !== 'intro-call'),
    [meetings]
  );


  const handleDelete = async (itemId: string) => {
    console.log('Deleting item:', itemId);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('meeting_delete', {
        meetingId: itemId,
      });
      console.log('Meeting deleted successfully');
      const updatedMeetings = meetings.filter((m: CurrentMeeting) => m.id !== itemId);
      setMeetings(updatedMeetings);

      // Track meeting deletion
      Analytics.trackMeetingDeleted(itemId);

      // Show success toast
      toast.success("Meeting deleted successfully", {
        description: "All associated data has been removed"
      });

      // If deleting the active meeting, navigate to home
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error("Failed to delete meeting", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleDeleteConfirm = () => {
    if (deleteModalState.itemId) {
      handleDelete(deleteModalState.itemId);
    }
    setDeleteModalState({ isOpen: false, itemId: null });
  };

  // Handle modal editing of meeting names
  const handleEditStart = (meetingId: string, currentTitle: string) => {
    setEditModalState({
      isOpen: true,
      meetingId: meetingId,
      currentTitle: currentTitle
    });
    setEditingTitle(currentTitle);
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;

    if (!meetingId) return;

    // Prevent empty titles
    if (!newTitle) {
      toast.error("Meeting title cannot be empty");
      return;
    }

    try {
      await invoke('meeting_title_set', {
        meetingId: meetingId,
        title: newTitle,
      });

      // Update local state
      const updatedMeetings = meetings.map((m: CurrentMeeting) =>
        m.id === meetingId ? { ...m, title: newTitle } : m
      );
      setMeetings(updatedMeetings);

      // Update current meeting if it's the one being edited
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }

      // Track the edit
      Analytics.trackButtonClick('edit_meeting_title', 'sidebar');

      toast.success("Meeting title updated successfully");

      // Close modal and reset state
      setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error("Failed to update meeting title", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleEditCancel = () => {
    setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
    setEditingTitle('');
  };

  const handleOpenBatchExport = () => {
    setIsBatchExportOpen(true);
    setBatchExportResults(null);
    setSelectedForBatchExport(new Set(batchExportCandidates.map((meeting) => meeting.id)));
  };

  const handleToggleBatchMeeting = (meetingId: string, checked: boolean) => {
    setSelectedForBatchExport((prev) => {
      const next = new Set(prev);
      if (checked) {
        next.add(meetingId);
      } else {
        next.delete(meetingId);
      }
      return next;
    });
  };

  const handlePickBatchFolder = async () => {
    try {
      const selected = await invoke<string | null>('select_recording_folder');
      if (selected) {
        setBatchDestinationRoot(selected);
      }
    } catch (error) {
      console.error('Failed to select batch export folder:', error);
      toast.error('Failed to select export destination');
    }
  };

  const handleBatchExport = async () => {
    if (selectedForBatchExport.size === 0) {
      toast.error('Select at least one meeting');
      return;
    }
    if (!batchDestinationRoot.trim()) {
      toast.error('Select a destination folder');
      return;
    }

    try {
      setIsBatchExporting(true);
      setBatchExportResults(null);
      const response = await invoke<{ results: BatchExportResult[] }>('meetings_export_markdown_batch', {
        meetingIds: Array.from(selectedForBatchExport),
        destinationRoot: batchDestinationRoot,
      });
      setBatchExportResults(response.results);

      const successCount = response.results.filter((result) => result.success).length;
      const failureCount = response.results.length - successCount;
      if (failureCount > 0) {
        toast.warning(`Exported ${successCount} meetings, ${failureCount} failed`);
      } else {
        toast.success(`Exported ${successCount} meetings`);
      }
    } catch (error) {
      console.error('Batch export failed:', error);
      toast.error(`Batch export failed: ${String(error)}`);
    } finally {
      setIsBatchExporting(false);
    }
  };

  const toggleFolder = (folderId: string) => {
    // Normal toggle behavior for all folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };

  // Expose setShowModelSettings to window for Rust tray to call
  useEffect(() => {
    const appWindow = window as Window & { openSettings?: () => void };
    appWindow.openSettings = () => {
      router.push('/settings');
    };

    // Cleanup on unmount
    return () => {
      delete appWindow.openSettings;
    };
  }, [router]);

  useEffect(() => {
    const loadVersion = async () => {
      try {
        const version = await getVersion();
        setAppVersion(version);
      } catch (error) {
        console.error('Failed to load app version:', error);
      }
    };
    void loadVersion();
  }, []);

  const renderCollapsedIcons = () => {
    if (!isCollapsed) return null;
    const navButtonClass = (active: boolean) =>
      `h-9 w-9 flex items-center justify-center rounded-lg transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 ${
        active ? 'bg-gray-100 text-gray-900' : 'hover:bg-gray-100 text-gray-700'
      }`;

    return (
      <TooltipProvider>
        <div className="flex flex-col items-center space-y-3 mt-3">
          <button
            type="button"
            onClick={toggleCollapse}
            className="bg-transparent border-none p-1 rounded-md hover:bg-gray-100/80 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
            aria-label="Expand sidebar"
          >
            <Logo isCollapsed={isCollapsed} />
          </button>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/')}
                className={navButtonClass(isHomePage)}
                aria-label="Go to home"
              >
                <Home className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Home</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={handleRecordingToggle}
                disabled={isRecording}
                className={`h-9 w-9 flex items-center justify-center ${isRecording ? 'bg-red-400 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600'} rounded-full transition-colors duration-150 shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400`}
                aria-label={isRecording ? 'Recording in progress' : 'Start recording'}
              >
                {isRecording ? (
                  <Square className="w-4 h-4 text-white" />
                ) : (
                  <Mic className="w-4 h-4 text-white" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>{isRecording ? "Recording in progress..." : "Start Recording"}</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => openImportDialog()}
                className="h-9 w-9 flex items-center justify-center rounded-lg transition-colors duration-150 hover:bg-blue-50 text-blue-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                aria-label="Import audio"
              >
                <Upload className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Import Audio</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={handleOpenBatchExport}
                className="h-9 w-9 flex items-center justify-center rounded-lg transition-colors duration-150 hover:bg-emerald-50 text-emerald-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500"
                aria-label="Batch export markdown"
              >
                <FileOutput className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Batch Export</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => {
                  if (isCollapsed) toggleCollapse();
                  toggleFolder('meetings');
                }}
                className={navButtonClass(isMeetingPage)}
                aria-label="Open meetings"
              >
                <NotebookPen className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Meeting Notes</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/settings')}
                className={navButtonClass(isSettingsPage)}
                aria-label="Open settings"
              >
                <Settings className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Settings</p>
            </TooltipContent>
          </Tooltip>

        </div>
      </TooltipProvider>
    );
  };

  // Find matching transcript snippet for a meeting item
  const findMatchingSnippet = (itemId: string) => {
    if (!searchQuery.trim() || !searchResults.length) return null;
    return searchResults.find(result => result.id === itemId);
  };

  const renderItem = (item: SidebarItem, depth = 0) => {
    const isExpanded = expandedFolders.has(item.id);
    const paddingLeft = `${depth * 12 + 12}px`;
    const isActive = item.type === 'file' && currentMeeting?.id === item.id;
    const isMeetingItem = item.type === 'file' && item.id !== 'intro-call';

    // Check if this item has a matching transcript snippet
    const matchingResult = isMeetingItem ? findMatchingSnippet(item.id) : null;
    const hasTranscriptMatch = !!matchingResult;

    if (isCollapsed) return null;

    return (
      <div key={item.id}>
        <div
          className={`flex items-center transition-all duration-150 group border ${item.type === 'folder' && depth === 0
            ? 'h-9 mx-3 mt-2 px-3 rounded-md border-transparent text-xs font-semibold uppercase tracking-wide text-gray-600'
            : `px-3 py-2 my-0.5 rounded-md text-sm ${
              isActive
                ? 'bg-blue-50 border-blue-200 text-blue-700 font-medium shadow-sm'
                : hasTranscriptMatch
                ? 'bg-yellow-50 border-yellow-200 hover:bg-yellow-100 hover:shadow-sm'
                : 'border-transparent hover:bg-gray-50 hover:border-gray-200'
              } cursor-pointer`
            }`}
          style={item.type === 'folder' && depth === 0 ? {} : { paddingLeft }}
          onClick={() => {
            if (item.type === 'folder') {
              toggleFolder(item.id);
            } else {
              setCurrentMeeting({ id: item.id, title: item.title });
              const basePath = item.id === 'intro-call' ? '/' : `/meeting-details?id=${item.id}`;
              router.push(basePath);
            }
          }}
        >
          {item.type === 'folder' ? (
            <>
              {item.id === 'meetings' ? (
                <Calendar className="w-4 h-4 mr-2" />
              ) : null}
              <span className={depth === 0 ? "" : "font-medium"}>{item.title}</span>
              <div className="ml-auto">
                {isExpanded ? (
                  <ChevronDown className="w-4 h-4 text-gray-500" />
                ) : (
                  <ChevronRight className="w-4 h-4 text-gray-500" />
                )}
              </div>
              {searchQuery && item.id === 'meetings' && isSearching && (
                <span className="ml-2 text-xs text-blue-500 animate-pulse">Searching...</span>
              )}
            </>
          ) : (
            <div className="flex flex-col w-full">
              <div className="flex items-center w-full">
                {isMeetingItem ? (
                  <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-gray-100">
                    <File className="w-3.5 h-3.5 text-gray-600" />
                  </div>
                ) : (
                  <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-blue-100">
                    <Plus className="w-3.5 h-3.5 text-blue-600" />
                  </div>
                )}
                <span className="flex-1 truncate">{item.title}</span>
                {isMeetingItem && (
                  <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity duration-200 ease-in-out">
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleEditStart(item.id, item.title);
                      }}
                      className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 transition-all duration-150"
                      aria-label="Edit meeting title"
                    >
                      <Pencil className="w-4 h-4" />
                    </button>
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        setDeleteModalState({ isOpen: true, itemId: item.id });
                      }}
                      className="hover:text-red-600 p-1 rounded-md hover:bg-red-50 flex-shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500 transition-all duration-150"
                      aria-label="Delete meeting"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                )}
              </div>

              {/* Show transcript match snippet if available */}
              {hasTranscriptMatch && (
                <div className="mt-2 ml-8 text-xs bg-yellow-50/80 backdrop-blur-sm p-2 rounded-md border border-yellow-200/50 shadow-sm">
                  <div className="flex items-start gap-1.5">
                    <SearchIcon className="w-3 h-3 text-yellow-600 mt-0.5 flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <span className="font-medium text-yellow-700 block mb-0.5">Match found:</span>
                      <p className="text-gray-600 line-clamp-2 leading-relaxed">
                        {matchingResult.matchContext}
                      </p>
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
        {item.type === 'folder' && isExpanded && item.children && (
          <div className="ml-1">
            {item.children.map(child => renderItem(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      <div
        className={`h-screen bg-white border-r shadow-sm flex flex-col transition-all duration-300 ${isCollapsed ? 'w-16' : 'w-64'
          }`}
      >
        <div className="flex-shrink-0">
          <div className="flex-1">
            {!isCollapsed && (
              <div className="px-3 pt-3 pb-2">
                <button
                  type="button"
                  onClick={toggleCollapse}
                  className="block w-full text-left rounded-md hover:bg-gray-100/80 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
                  aria-label="Collapse sidebar"
                >
                  <Logo isCollapsed={isCollapsed} />
                </button>

                <div className="relative mt-2">
                  <InputGroup >
                    <InputGroupInput
                      aria-label="Search meeting content"
                      placeholder='Search meeting content...'
                      value={searchQuery}
                      onChange={(e) => handleSearchChange(e.target.value)}
                      disabled={isSearching}
                    />
                    <InputGroupAddon>
                      {isSearching ? (
                        <div className="animate-spin">
                          <SearchIcon className="w-4 h-4" />
                        </div>
                      ) : (
                        <SearchIcon />
                      )}
                    </InputGroupAddon>
                    {searchQuery &&
                      <InputGroupAddon align={'inline-end'}>
                        <InputGroupButton
                          aria-label="Clear search"
                          onClick={() => handleSearchChange('')}
                          disabled={isSearching}
                        >
                          <X />
                        </InputGroupButton>
                      </InputGroupAddon>
                    }
                  </InputGroup>
                </div>

                <div className="mt-2 grid grid-cols-2 gap-2">
                  <input
                    type="date"
                    value={searchFilters.dateFrom}
                    onChange={(e) => void handleFilterChange({ dateFrom: e.target.value })}
                    className="h-8 rounded border border-gray-200 px-2 text-xs text-gray-700"
                    aria-label="Filter from date"
                  />
                  <input
                    type="date"
                    value={searchFilters.dateTo}
                    onChange={(e) => void handleFilterChange({ dateTo: e.target.value })}
                    className="h-8 rounded border border-gray-200 px-2 text-xs text-gray-700"
                    aria-label="Filter to date"
                  />
                  <select
                    value={searchFilters.sourceType}
                    onChange={(e) => void handleFilterChange({ sourceType: e.target.value as typeof searchFilters.sourceType })}
                    className="h-8 rounded border border-gray-200 px-2 text-xs text-gray-700 bg-white"
                    aria-label="Filter source type"
                  >
                    <option value="all">All Sources</option>
                    <option value="recorded">Recorded</option>
                    <option value="imported">Imported</option>
                    <option value="retranscribed">Retranscribed</option>
                  </select>
                  <select
                    value={searchFilters.hasSummary}
                    onChange={(e) => void handleFilterChange({ hasSummary: e.target.value as typeof searchFilters.hasSummary })}
                    className="h-8 rounded border border-gray-200 px-2 text-xs text-gray-700 bg-white"
                    aria-label="Filter summary availability"
                  >
                    <option value="all">All Summaries</option>
                    <option value="yes">With Summary</option>
                    <option value="no">No Summary</option>
                  </select>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Main content - scrollable area */}
        <div className="flex-1 flex flex-col min-h-0">
          {/* Fixed navigation items */}
          <div className="flex-shrink-0">
            {!isCollapsed && (
              <button
                type="button"
                onClick={() => router.push('/')}
                className={`w-[calc(100%-1.5rem)] px-3 h-9 text-sm font-semibold items-center flex mx-3 mt-2 rounded-md transition-colors ${
                  isHomePage ? 'bg-gray-100 text-gray-900' : 'hover:bg-gray-100 text-gray-700'
                } focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500`}
                aria-label="Go to home"
              >
                <Home className="w-4 h-4 mr-2" />
                <span>Home</span>
              </button>
            )}
          </div>

          {/* Content area */}
          <div className="flex-1 flex flex-col min-h-0">
            {renderCollapsedIcons()}
            {/* Meeting Notes folder header - fixed */}
            {!isCollapsed && (
              <div className="flex-shrink-0">
                {filteredSidebarItems.filter(item => item.type === 'folder').map(item => (
                  <div key={item.id}>
                    <div
                      className="flex items-center transition-all duration-150 h-8 mx-3 mt-2 px-3 rounded-md text-xs font-semibold uppercase tracking-wide text-gray-600"
                    >
                      <NotebookPen className="w-3.5 h-3.5 mr-2 text-gray-500" />
                      <span>{item.title}</span>
                      {searchQuery && item.id === 'meetings' && isSearching && (
                        <span className="ml-2 text-xs text-blue-500 animate-pulse">Searching...</span>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Scrollable meeting items */}
            {!isCollapsed && (
              <div className="flex-1 overflow-y-auto custom-scrollbar min-h-0">
                {hasNoSearchResults ? (
                  <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
                    <SearchIcon className="w-12 h-12 text-gray-300 mb-3" />
                    <p className="text-sm font-medium text-gray-600">No results found</p>
                    <p className="text-xs text-gray-400 mt-1">Try a different search term</p>
                  </div>
                ) : (
                  filteredSidebarItems
                    .filter(item => item.type === 'folder' && expandedFolders.has(item.id) && item.children)
                    .map(item => (
                      <div key={`${item.id}-children`} className="mx-3">
                        {item.children!.map(child => renderItem(child, 1))}
                      </div>
                    ))
                )}
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        {!isCollapsed && (

          <div className="flex-shrink-0 p-3 border-t border-gray-100 space-y-2">
            <button
              type="button"
              onClick={handleRecordingToggle}
              disabled={isRecording}
              className={`w-full flex items-center justify-center px-4 py-2.5 text-sm font-semibold text-white ${isRecording ? 'bg-red-400 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600 active:bg-red-700'} rounded-lg transition-all duration-150 shadow-md hover:shadow-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400 focus-visible:ring-offset-2`}
            >
              {isRecording ? (
                <>
                  <Square className="w-4 h-4 mr-2" />
                  <span>Recording in progress...</span>
                </>
              ) : (
                <>
                  <Mic className="w-4 h-4 mr-2" />
                  <span>Start Recording</span>
                </>
              )}
            </button>

            <button
              onClick={() => openImportDialog()}
              className="w-full flex items-center justify-center px-3 py-2 text-sm font-medium text-blue-700 bg-blue-50 hover:bg-blue-100 active:bg-blue-200 rounded-lg transition-all duration-150 border border-blue-200"
            >
              <Upload className="w-4 h-4 mr-2" />
              <span>Import Audio</span>
            </button>

            <button
              type="button"
              onClick={handleOpenBatchExport}
              className="w-full flex items-center justify-center px-3 py-2 text-sm font-medium text-emerald-700 bg-emerald-50 hover:bg-emerald-100 active:bg-emerald-200 rounded-lg transition-all duration-150 border border-emerald-200"
            >
              <FileOutput className="w-4 h-4 mr-2" />
              <span>Batch Export</span>
            </button>

            <button
              type="button"
              onClick={() => router.push('/settings')}
              className={`w-full flex items-center justify-center px-3 py-2 text-sm font-medium rounded-lg transition-all duration-150 ${
                isSettingsPage
                  ? 'text-gray-900 bg-gray-100 border border-gray-300'
                  : 'text-gray-600 bg-transparent hover:bg-gray-50 border border-transparent'
              } focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500`}
              aria-label="Open settings"
            >
              <Settings className="w-4 h-4 mr-2" />
              <span>Settings</span>
            </button>
            <div className="w-full flex items-center justify-center px-3 py-1 text-xs text-gray-400 font-mono">
              {appVersion ? `v${appVersion}` : 'v...'}
            </div>
          </div>
        )}
      </div>

      {/* Confirmation Modal for Delete */}
      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Are you sure you want to delete this meeting? This action cannot be undone."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      <Dialog open={isBatchExportOpen} onOpenChange={setIsBatchExportOpen}>
        <DialogContent className="sm:max-w-[680px]">
          <DialogTitle>Batch Export Markdown</DialogTitle>
          <div className="space-y-4 py-2">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Destination Root Folder
              </label>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={batchDestinationRoot}
                  onChange={(e) => setBatchDestinationRoot(e.target.value)}
                  placeholder="Select destination folder"
                  className="flex-1 h-9 rounded border border-gray-300 px-3 text-sm"
                />
                <button
                  type="button"
                  onClick={() => void handlePickBatchFolder()}
                  className="h-9 px-3 rounded border border-gray-300 text-sm hover:bg-gray-50"
                >
                  Browse
                </button>
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-sm font-medium text-gray-700">Meetings</label>
                <button
                  type="button"
                  onClick={() => setSelectedForBatchExport(new Set(batchExportCandidates.map((meeting) => meeting.id)))}
                  className="text-xs text-blue-600 hover:text-blue-700"
                >
                  Select all
                </button>
              </div>
              <div className="max-h-64 overflow-y-auto border rounded-md divide-y">
                {batchExportCandidates.length === 0 ? (
                  <div className="p-3 text-sm text-gray-500">No meetings available.</div>
                ) : (
                  batchExportCandidates.map((meeting) => (
                    <label key={meeting.id} className="flex items-center justify-between gap-3 p-3 text-sm">
                      <span className="truncate">{meeting.title}</span>
                      <input
                        type="checkbox"
                        checked={selectedForBatchExport.has(meeting.id)}
                        onChange={(e) => handleToggleBatchMeeting(meeting.id, e.target.checked)}
                      />
                    </label>
                  ))
                )}
              </div>
            </div>

            {batchExportResults && (
              <div className="border rounded-md divide-y">
                {batchExportResults.map((result) => {
                  const meeting = batchExportCandidates.find((candidate) => candidate.id === result.meeting_id);
                  return (
                    <div key={result.meeting_id} className="p-3 text-sm">
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-medium truncate">{meeting?.title ?? result.meeting_id}</span>
                        <span className={result.success ? "text-green-600" : "text-red-600"}>
                          {result.success ? "Success" : "Failed"}
                        </span>
                      </div>
                      {result.output_path && (
                        <p className="text-xs text-gray-500 truncate mt-1">{result.output_path}</p>
                      )}
                      {result.error && (
                        <p className="text-xs text-red-600 mt-1">{result.error}</p>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          <DialogFooter>
            <button
              type="button"
              onClick={() => setIsBatchExportOpen(false)}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Close
            </button>
            <button
              type="button"
              onClick={() => void handleBatchExport()}
              disabled={isBatchExporting}
              className="px-4 py-2 text-sm font-medium text-white bg-emerald-600 hover:bg-emerald-700 disabled:opacity-60 rounded-md transition-colors"
            >
              {isBatchExporting ? 'Exporting...' : 'Export Markdown'}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Meeting Title Modal */}
      <Dialog open={editModalState.isOpen} onOpenChange={(open) => {
        if (!open) handleEditCancel();
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Edit Meeting Title</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">Edit Meeting Title</h3>
            <div className="space-y-4">
              <div>
                <label htmlFor="meeting-title" className="block text-sm font-medium text-gray-700 mb-2">
                  Meeting Title
                </label>
                <input
                  id="meeting-title"
                  type="text"
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleEditConfirm();
                    } else if (e.key === 'Escape') {
                      handleEditCancel();
                    }
                  }}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                  placeholder="Enter meeting title"
                  autoFocus
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={handleEditCancel}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleEditConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;

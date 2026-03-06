'use client';

import React, { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { authFetch } from '@/lib/api';
import { toast } from 'sonner';

interface ShareNotesDialogProps {
  isOpen: boolean;
  meetingId: string;
  onClose: () => void;
  onShared?: () => void;
}

export function ShareNotesDialog({ isOpen, meetingId, onClose, onShared }: ShareNotesDialogProps) {
  const [shareSummary, setShareSummary] = useState(true);
  const [shareTranscript, setShareTranscript] = useState(false);
  const [isSharing, setIsSharing] = useState(false);

  const handleShare = async () => {
    try {
      setIsSharing(true);
      const res = await authFetch(`/api/sharing/${meetingId}/share`, {
        method: 'POST',
        body: JSON.stringify({
          meeting_id: meetingId,
          share_summary: shareSummary,
          share_transcript: shareTranscript,
          // recipient_emails: null implies "all accepted attendees" handled by backend
        }),
      });
      
      if (res.ok) {
        toast.success('Notes shared successfully');
        if (onShared) onShared();
        onClose();
      } else {
        throw new Error('Failed to share notes');
      }
    } catch (error) {
      console.error(error);
      toast.error('Could not share notes');
    } finally {
      setIsSharing(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Share Updated Notes?</DialogTitle>
          <DialogDescription>
            You just generated new notes for this meeting. Would you like to share them with the attendees?
          </DialogDescription>
        </DialogHeader>
        
        <div className="grid gap-4 py-4">
          <div className="flex items-center justify-between space-x-2 border p-3 rounded-lg">
            <div className="flex flex-col space-y-1">
              <Label htmlFor="share-summary">Share Summary</Label>
              <span className="text-sm text-gray-500">Includes TL;DR and Action Items</span>
            </div>
            <Switch
              id="share-summary"
              checked={shareSummary}
              onCheckedChange={setShareSummary}
            />
          </div>
          
          <div className="flex items-center justify-between space-x-2 border p-3 rounded-lg">
            <div className="flex flex-col space-y-1">
              <Label htmlFor="share-transcript">Share Transcript</Label>
              <span className="text-sm text-gray-500">Full conversation text</span>
            </div>
            <Switch
              id="share-transcript"
              checked={shareTranscript}
              onCheckedChange={setShareTranscript}
            />
          </div>
        </div>
        
        <DialogFooter className="flex space-x-2 sm:space-x-0">
          <button 
            onClick={onClose}
            className="px-4 py-2 border rounded-md text-gray-700 hover:bg-gray-50 flex-1"
            disabled={isSharing}
          >
            Skip Sharing
          </button>
          <button 
            onClick={handleShare}
            className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 flex-1"
            disabled={isSharing}
          >
            {isSharing ? 'Sharing...' : 'Share Notes'}
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

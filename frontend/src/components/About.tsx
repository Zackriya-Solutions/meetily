import React, { useState, useEffect } from "react";
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import Image from 'next/image';
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch";
import { UpdateDialog } from "./UpdateDialog";
import { updateService, UpdateInfo } from '@/services/updateService';
import { Button } from './ui/button';
import { Loader2, CheckCircle2 } from 'lucide-react';
import { toast } from 'sonner';


export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('0.5.0');
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
    const [isChecking, setIsChecking] = useState(false);
    const [showUpdateDialog, setShowUpdateDialog] = useState(false);

    useEffect(() => {
        // Get current version on mount
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    const handleSourceClick = async () => {
        try {
            await invoke('open_external_url', { url: 'https://github.com/MashdorDev/open-meeting-notes' });
        } catch (error) {
            console.error('Failed to open link:', error);
        }
    };

    const handleCheckForUpdates = async () => {
        setIsChecking(true);
        try {
            const info = await updateService.checkForUpdates(true);
            setUpdateInfo(info);
            if (info.available) {
                setShowUpdateDialog(true);
            } else {
                toast.success('You are running the latest version');
            }
        } catch (error: any) {
            console.error('Failed to check for updates:', error);
            toast.error('Failed to check for updates: ' + (error.message || 'Unknown error'));
        } finally {
            setIsChecking(false);
        }
    };

    return (
        <div className="p-4 space-y-4 h-[80vh] overflow-y-auto">
            {/* Compact Header */}
            <div className="text-center">
                <div className="mb-3">
                    <Image
                        src="icon_128x128.png"
                        alt=""
                        width={64}
                        height={64}
                        className="mx-auto"
                    />
                </div>
                <h1 className="font-heading text-xl font-semibold text-gray-900">Open Meeting Notes</h1>
                <span className="text-sm text-gray-500"> v{currentVersion}</span>
                <p className="text-medium text-gray-600 mt-1">
                    Recordings, transcripts and summaries that stay on your machine.
                </p>
                <div className="mt-3">
                    <Button
                        onClick={handleCheckForUpdates}
                        disabled={isChecking}
                        variant="outline"
                        size="sm"
                        className="text-xs"
                    >
                        {isChecking ? (
                            <>
                                <Loader2 className="h-3 w-3 mr-2 animate-spin" />
                                Checking...
                            </>
                        ) : (
                            <>
                                <CheckCircle2 className="h-3 w-3 mr-2" />
                                Check for Updates
                            </>
                        )}
                    </Button>
                    {updateInfo?.available && (
                        <div className="mt-2 text-xs text-blue-600">
                            Update available: v{updateInfo.version}
                        </div>
                    )}
                </div>
            </div>

            {/* Features Grid - Compact */}
            <div className="space-y-3">
                <h2 className="font-heading text-base font-semibold text-gray-800">What it does</h2>
                <ul className="space-y-2 text-sm text-gray-700 leading-relaxed list-disc pl-5">
                    <li>Transcribes calls on this machine, with Me and the other side labelled, and each remote voice as its own speaker.</li>
                    <li>Keeps the recording, so you can play any meeting back.</li>
                    <li>Summarizes with a local model or the API you choose.</li>
                </ul>
            </div>

            <div className="pt-2 border-t border-gray-200 text-center space-y-1">
                <button
                    onClick={handleSourceClick}
                    className="text-sm font-medium text-blue-700 hover:text-blue-800 underline-offset-2 hover:underline"
                >
                    Source code on GitHub
                </button>
                <p className="text-xs text-gray-500">
                    Based on Meetily by Zackriya Solutions (MIT license).
                </p>
            </div>
            <AnalyticsConsentSwitch />

            {/* Update Dialog */}
            <UpdateDialog
                open={showUpdateDialog}
                onOpenChange={setShowUpdateDialog}
                updateInfo={updateInfo}
            />
        </div>

    )
}
import React, { useState, useEffect } from "react";
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch";
import { UpdateDialog } from "./UpdateDialog";
import { updateService, UpdateInfo } from '@/services/updateService';
import { Button } from './ui/button';
import { Loader2, CheckCircle2 } from 'lucide-react';
import { toast } from 'sonner';
import { ClearMinutesIcon } from "./ClearMinutesLogo";


export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('0.3.0');
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
    const [isChecking, setIsChecking] = useState(false);
    const [showUpdateDialog, setShowUpdateDialog] = useState(false);

    useEffect(() => {
        // Get current version on mount
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    const handleContactClick = async () => {
        try {
            await invoke('open_external_url', { url: 'https://clearminutes.app/#contact' });
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
            {/* Header */}
            <div className="text-center">
                <div className="mb-3 flex justify-center">
                    <ClearMinutesIcon size={64} />
                </div>
                <h1 className="text-xl font-bold text-gray-900 dark:text-foreground"
                    style={{ fontFamily: 'var(--font-syne, system-ui)', letterSpacing: '-0.02em' }}>
                    clearminutes
                </h1>
                <span className="text-sm text-gray-500 dark:text-muted-foreground"> v{currentVersion}</span>
                <p className="text-sm text-gray-600 dark:text-muted-foreground mt-1">
                    Real-time notes and summaries that never leave your machine.
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
                            <><Loader2 className="h-3 w-3 mr-2 animate-spin" />Checking...</>
                        ) : (
                            <><CheckCircle2 className="h-3 w-3 mr-2" />Check for Updates</>
                        )}
                    </Button>
                    {updateInfo?.available && (
                        <div className="mt-2 text-xs" style={{ color: 'hsl(var(--theme-accent))' }}>
                            Update available: v{updateInfo.version}
                        </div>
                    )}
                </div>
            </div>

            {/* Features Grid */}
            <div className="space-y-3">
                <h2 className="text-base font-semibold text-gray-800 dark:text-foreground">What makes Clearminutes different</h2>
                <div className="grid grid-cols-2 gap-2">
                    <div className="bg-gray-50 dark:bg-secondary rounded p-3 hover:bg-gray-100 dark:hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-gray-900 dark:text-foreground mb-1">Privacy-first</h3>
                        <p className="text-xs text-gray-600 dark:text-muted-foreground leading-relaxed">Your data &amp; AI processing workflow can now stay within your premise. No cloud, no leaks.</p>
                    </div>
                    <div className="bg-gray-50 dark:bg-secondary rounded p-3 hover:bg-gray-100 dark:hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-gray-900 dark:text-foreground mb-1">Use Any Model</h3>
                        <p className="text-xs text-gray-600 dark:text-muted-foreground leading-relaxed">Prefer local open-source model? Great. Want to plug in an external API? Also fine. No lock-in.</p>
                    </div>
                    <div className="bg-gray-50 dark:bg-secondary rounded p-3 hover:bg-gray-100 dark:hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-gray-900 dark:text-foreground mb-1">Cost-Smart</h3>
                        <p className="text-xs text-gray-600 dark:text-muted-foreground leading-relaxed">Avoid pay-per-minute bills by running models locally (or pay only for the calls you choose).</p>
                    </div>
                    <div className="bg-gray-50 dark:bg-secondary rounded p-3 hover:bg-gray-100 dark:hover:bg-accent transition-colors">
                        <h3 className="font-bold text-sm text-gray-900 dark:text-foreground mb-1">Works everywhere</h3>
                        <p className="text-xs text-gray-600 dark:text-muted-foreground leading-relaxed">Google Meet, Zoom, Teams — online or offline.</p>
                    </div>
                </div>
            </div>

            {/* Coming soon */}
            <div className="bg-blue-50 dark:bg-secondary rounded p-3">
                <p className="text-s text-blue-800 dark:text-primary">
                    <span className="font-bold">Coming soon:</span> A library of on-device AI agents — automating follow-ups, action tracking, and more.
                </p>
            </div>

            {/* CTA */}
            <div className="text-center space-y-2">
                <h3 className="text-medium font-semibold text-gray-800 dark:text-foreground">Ready to push your business further?</h3>
                <p className="text-s text-gray-600 dark:text-muted-foreground">
                    Planning to build privacy-first custom AI agents or a fully tailored product for your <span className="font-bold">business</span>? We can help.
                </p>
                <button
                    onClick={handleContactClick}
                    className="inline-flex items-center px-4 py-2 text-white text-sm font-medium rounded transition-colors duration-200 shadow-sm hover:opacity-90"
                    style={{ backgroundColor: 'hsl(var(--theme-accent))' }}
                >
                    Chat with the DevBytes team
                </button>
            </div>

            {/* Footer */}
            <div className="pt-2 border-t border-gray-200 dark:border-border text-center">
                <p className="text-xs text-gray-400 dark:text-muted-foreground">
                    Built by DevBytes
                </p>
            </div>
            <AnalyticsConsentSwitch />

            <UpdateDialog
                open={showUpdateDialog}
                onOpenChange={setShowUpdateDialog}
                updateInfo={updateInfo}
            />
        </div>
    );
}
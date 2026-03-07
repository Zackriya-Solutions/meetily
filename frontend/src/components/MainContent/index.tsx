"use client";

import React, { useEffect } from "react";
import { useSidebar } from "@/components/Sidebar/SidebarProvider";

interface MainContentProps {
  children: React.ReactNode;
}

const MainContent: React.FC<MainContentProps> = ({ children }) => {
  const { isCollapsed } = useSidebar();

  useEffect(() => {
    const handleMouseDown = async (e: MouseEvent) => {
      // Only trigger on primary button in the top 32px (titlebar zone)
      if (e.buttons !== 1 || e.clientY > 32) return;
      // Don't drag if the target is an interactive element
      const target = e.target as HTMLElement;
      if (target.closest('button, a, input, textarea, select, [role="button"]')) return;
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const appWindow = getCurrentWindow();
        e.detail === 2 ? appWindow.toggleMaximize() : appWindow.startDragging();
      } catch {}
    };
    document.addEventListener('mousedown', handleMouseDown);
    return () => document.removeEventListener('mousedown', handleMouseDown);
  }, []);

  return (
    <main
      className={`flex-1 transition-all duration-300 ${
        isCollapsed ? "ml-16" : "ml-64"
      }`}
    >
      {children}
    </main>
  );
};

export default MainContent;

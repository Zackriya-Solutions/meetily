"use client";

import { useEffect } from "react";
import { useTheme } from "next-themes";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Toaster } from "sonner";

export function ThemeEffects() {
  const { theme, resolvedTheme } = useTheme();

  useEffect(() => {
    if (isTauri() && theme) {
      getCurrentWindow().setTheme(theme === "system" ? null : theme === "dark" ? "dark" : "light")
        .catch((error) => console.error("Failed to update window appearance:", error));
    }
  }, [theme]);

  return <Toaster theme={resolvedTheme === "dark" ? "dark" : "light"} position="bottom-center" richColors closeButton />;
}

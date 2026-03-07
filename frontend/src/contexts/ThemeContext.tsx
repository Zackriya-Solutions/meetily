'use client';

import React, { createContext, useContext, useEffect, useState } from 'react';

export type ThemeVariant = 'light' | 'dark' | 'dracula' | 'midnight';

export const THEME_OPTIONS: { value: ThemeVariant; label: string; description: string; preview: string[] }[] = [
  { value: 'light',    label: 'Light',    description: 'Clean white interface',         preview: ['#ffffff', '#f3f4f6', '#00d4aa'] },
  { value: 'dark',     label: 'Dark',     description: 'Soft dark, easy on the eyes',  preview: ['#1e2028', '#252830', '#00d4aa'] },
  { value: 'dracula',  label: 'Dracula',  description: 'Purple-tinted Dracula palette', preview: ['#282a36', '#44475a', '#bd93f9'] },
  { value: 'midnight', label: 'Midnight', description: 'Deep blue-black night theme',   preview: ['#0f1117', '#161b27', '#60a5fa'] },
];

interface ThemeContextType {
  theme: ThemeVariant;
  setTheme: (t: ThemeVariant) => void;
  isDark: boolean;
}

const ThemeContext = createContext<ThemeContextType>({
  theme: 'light',
  setTheme: () => {},
  isDark: false,
});

function applyTheme(theme: ThemeVariant) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  // Remove all theme data attrs, then set current
  root.removeAttribute('data-theme');
  root.classList.remove('dark');
  if (theme !== 'light') {
    root.classList.add('dark');
    root.setAttribute('data-theme', theme);
  }
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<ThemeVariant>('light');

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const stored = localStorage.getItem('clearminutes-theme') as ThemeVariant | null;
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    const initial: ThemeVariant = stored ?? (prefersDark ? 'dark' : 'light');
    setThemeState(initial);
    applyTheme(initial);
  }, []);

  const setTheme = (next: ThemeVariant) => {
    setThemeState(next);
    if (typeof window !== 'undefined') {
      localStorage.setItem('clearminutes-theme', next);
      applyTheme(next);
    }
  };

  return (
    <ThemeContext.Provider value={{ theme, setTheme, isDark: theme !== 'light' }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}

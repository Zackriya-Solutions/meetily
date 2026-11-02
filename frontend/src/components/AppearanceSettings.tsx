"use client";

import { useEffect, useState } from "react";
import { useTheme } from "next-themes";

export function AppearanceSettings() {
  const { theme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);

  useEffect(() => setMounted(true), []);

  return (
    <section className="rounded-lg border border-border bg-card p-6 shadow-sm" aria-labelledby="appearance-heading">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h3 id="appearance-heading" className="mb-2 text-lg font-semibold text-foreground">Appearance</h3>
          <p className="text-sm text-muted-foreground">Use your system appearance or choose a theme.</p>
        </div>
        <select
          aria-label="Appearance"
          value={mounted ? theme : "system"}
          disabled={!mounted}
          onChange={(event) => setTheme(event.target.value)}
          className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <option value="system">System</option>
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </div>
    </section>
  );
}

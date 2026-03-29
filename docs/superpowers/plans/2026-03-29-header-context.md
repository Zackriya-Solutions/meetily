# HeaderContext Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the dual-layer heading system (generic AppHeader + per-page `<h1>`) with a single context-driven AppHeader that shows page-specific titles.

**Architecture:** A React context (`HeaderContext`) lets page components declaratively set AppHeader props via a `useHeader()` hook. The layout renders one `AppHeader` that reads from this context. Pages no longer render their own headings.

**Tech Stack:** React 18 Context API, TypeScript, Vitest, Next.js 14

**Spec:** `docs/superpowers/specs/2026-03-29-header-context-design.md`

---

### Task 1: Create HeaderContext

**Files:**
- Create: `mobile/src/contexts/HeaderContext.tsx`
- Create: `mobile/src/contexts/HeaderContext.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `mobile/src/contexts/HeaderContext.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import React from 'react'
import { HeaderProvider, useHeader, useHeaderContext } from './HeaderContext'

function wrapper({ children }: { children: React.ReactNode }) {
  return <HeaderProvider>{children}</HeaderProvider>
}

describe('HeaderContext', () => {
  it('provides default header values', () => {
    const { result } = renderHook(() => useHeaderContext(), { wrapper })
    expect(result.current.title).toBe('IQ:capture')
    expect(result.current.showBack).toBe(false)
    expect(result.current.subtitle).toBeUndefined()
    expect(result.current.rightContent).toBeUndefined()
  })

  it('useHeader sets header state on mount', () => {
    function useBoth() {
      useHeader({ title: 'Meetings', showBack: true })
      return useHeaderContext()
    }
    const { result } = renderHook(() => useBoth(), { wrapper })
    expect(result.current.title).toBe('Meetings')
    expect(result.current.showBack).toBe(true)
  })

  it('useHeader resets to defaults on unmount', () => {
    // First, mount a component that sets header
    const { unmount: unmountSetter } = renderHook(
      () => useHeader({ title: 'Settings' }),
      { wrapper },
    )
    const { result } = renderHook(() => useHeaderContext(), { wrapper })

    // After unmount, header resets to defaults
    act(() => { unmountSetter() })
    expect(result.current.title).toBe('IQ:capture')
    expect(result.current.showBack).toBe(false)
  })

  it('useHeader updates when props change', () => {
    let title = 'Loading...'
    const { result, rerender } = renderHook(
      () => {
        useHeader({ title })
        return useHeaderContext()
      },
      { wrapper },
    )
    expect(result.current.title).toBe('Loading...')

    title = 'Team Standup'
    rerender()
    expect(result.current.title).toBe('Team Standup')
  })

  it('throws when useHeaderContext is used outside provider', () => {
    expect(() => {
      renderHook(() => useHeaderContext())
    }).toThrow('useHeaderContext must be used within HeaderProvider')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx vitest run src/contexts/HeaderContext.test.tsx`
Expected: FAIL — module `./HeaderContext` not found

- [ ] **Step 3: Write minimal implementation**

Create `mobile/src/contexts/HeaderContext.tsx`:

```tsx
'use client'

import React, { createContext, useContext, useState, useEffect, useCallback, ReactNode } from 'react'

interface HeaderState {
  title: string
  subtitle?: string
  showBack: boolean
  rightContent?: ReactNode
}

const DEFAULTS: HeaderState = {
  title: 'IQ:capture',
  showBack: false,
}

interface HeaderContextValue extends HeaderState {
  setHeader: (state: Partial<HeaderState>) => void
  resetHeader: () => void
}

const HeaderContext = createContext<HeaderContextValue | null>(null)

export function useHeaderContext(): HeaderState {
  const ctx = useContext(HeaderContext)
  if (!ctx) throw new Error('useHeaderContext must be used within HeaderProvider')
  return ctx
}

export function useHeader(props: Partial<HeaderState>): void {
  const ctx = useContext(HeaderContext)
  if (!ctx) throw new Error('useHeader must be used within HeaderProvider')

  useEffect(() => {
    ctx.setHeader(props)
    return () => { ctx.resetHeader() }
  }, [props.title, props.subtitle, props.showBack, props.rightContent])
}

export function HeaderProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<HeaderState>(DEFAULTS)

  const setHeader = useCallback((partial: Partial<HeaderState>) => {
    setState({ ...DEFAULTS, ...partial })
  }, [])

  const resetHeader = useCallback(() => {
    setState(DEFAULTS)
  }, [])

  return (
    <HeaderContext.Provider value={{ ...state, setHeader, resetHeader }}>
      {children}
    </HeaderContext.Provider>
  )
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx vitest run src/contexts/HeaderContext.test.tsx`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/src/contexts/HeaderContext.tsx mobile/src/contexts/HeaderContext.test.tsx
git commit -m "feat: add HeaderContext for unified page header state"
```

---

### Task 2: Wire AppHeader to HeaderContext

**Files:**
- Modify: `mobile/src/components/AppHeader.tsx` (full rewrite — 49 lines)
- Modify: `mobile/src/app/layout.tsx:1-17` (imports), `layout.tsx:38-39` (auth wrapper), `layout.tsx:82-94` (main wrapper)

- [ ] **Step 1: Rewrite AppHeader to read from context**

Replace the entire contents of `mobile/src/components/AppHeader.tsx` with:

```tsx
'use client'

import React from 'react'
import { ChevronLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'
import { useHeaderContext } from '@/contexts/HeaderContext'

export default function AppHeader() {
  const router = useRouter()
  const { title, subtitle, showBack, rightContent } = useHeaderContext()

  return (
    <header
      className="sticky top-0 z-40 w-full flex-shrink-0 text-white"
      style={{
        background: 'linear-gradient(135deg, #2276aa, #1caac9)',
        paddingTop: 'calc(12px + env(safe-area-inset-top, 0px))',
        paddingBottom: '12px',
        paddingLeft: '16px',
        paddingRight: '16px',
      }}
    >
      <div className="flex items-center gap-3">
        {showBack && (
          <button onClick={() => router.back()} className="p-1 -ml-1 rounded-lg active:bg-white/10">
            <ChevronLeft className="w-5 h-5" />
          </button>
        )}
        <div className="flex-1 min-w-0">
          <h1 className="text-lg font-bold leading-tight truncate">{title}</h1>
          {subtitle && (
            <p className="text-xs text-white/60 truncate">{subtitle}</p>
          )}
        </div>
        {rightContent}
      </div>
    </header>
  )
}
```

- [ ] **Step 2: Add HeaderProvider to layout.tsx**

In `mobile/src/app/layout.tsx`, add the import at line 8 (after the other context imports):

```tsx
import { HeaderProvider } from '@/contexts/HeaderContext'
```

- [ ] **Step 3: Wrap the authenticated path with HeaderProvider**

In `mobile/src/app/layout.tsx`, replace lines 82-94 (the authenticated return block):

```tsx
  return (
    <SyncProvider>
      <RecordingProvider>
        <HeaderProvider>
          <main className="flex flex-col h-screen w-full overflow-x-hidden">
            <AppHeader />
            <div className="flex-1 overflow-y-auto overflow-x-hidden pb-16 px-4">
              {children}
            </div>
            <TabBar />
          </main>
        </HeaderProvider>
      </RecordingProvider>
    </SyncProvider>
  )
```

- [ ] **Step 4: Wrap the unauthenticated auth path with HeaderProvider**

In `mobile/src/app/layout.tsx`, replace lines 37-79 (the `!isAuthenticated` return block):

```tsx
    return (
      <HeaderProvider>
        <main className="flex flex-col h-screen bg-iq-light">
          <AppHeader />
          <div className="flex-1 overflow-y-auto">
            {authScreen === 'prompt' && (
              <AuthPrompt onNavigate={(page) => setAuthScreen(page)} />
            )}
            {authScreen === 'login' && (
              <InlineLoginForm
                onForgot={() => setAuthScreen('forgot')}
                onRegister={() => setAuthScreen('register')}
                onNeedsVerification={(email) => { setAuthEmail(email); setAuthScreen('verify') }}
              />
            )}
            {authScreen === 'register' && (
              <InlineRegisterForm
                onLogin={() => setAuthScreen('login')}
                onNeedsVerification={(email) => { setAuthEmail(email); setAuthScreen('verify') }}
              />
            )}
            {authScreen === 'forgot' && (
              <InlineForgotForm
                onBack={() => setAuthScreen('login')}
                onCodeSent={(email) => { setAuthEmail(email); setAuthScreen('reset') }}
              />
            )}
            {authScreen === 'verify' && (
              <InlineVerifyForm
                email={authEmail}
                onVerified={() => setAuthScreen('login')}
                onBack={() => setAuthScreen('login')}
              />
            )}
            {authScreen === 'reset' && (
              <InlineResetForm
                email={authEmail}
                onReset={() => setAuthScreen('login')}
                onBack={() => setAuthScreen('forgot')}
              />
            )}
          </div>
        </main>
      </HeaderProvider>
    )
```

- [ ] **Step 5: Remove AppHeader props import that is no longer needed**

In `mobile/src/app/layout.tsx`, the `<AppHeader title="IQ:capture" />` call on the auth path (previously line 39) is now just `<AppHeader />`. Verify no `title` prop is passed anywhere in layout.tsx.

- [ ] **Step 6: Verify the app compiles**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 7: Commit**

```bash
git add mobile/src/components/AppHeader.tsx mobile/src/app/layout.tsx
git commit -m "feat: wire AppHeader to HeaderContext, add HeaderProvider to layout"
```

---

### Task 3: Migrate MeetingsList to useHeader

**Files:**
- Modify: `mobile/src/components/MeetingsList.tsx:1-11` (imports), `48-61` (remove heading block)

- [ ] **Step 1: Add useHeader import and call**

In `mobile/src/components/MeetingsList.tsx`, add the import after line 7:

```tsx
import { useHeader } from '@/contexts/HeaderContext'
```

- [ ] **Step 2: Add useHeader call inside the component**

Inside the `MeetingsList` component, after line 15 (`const { forceSync, isSyncing } = useSync()`), add:

```tsx
  useHeader({
    title: 'Meetings',
    rightContent: (
      <button
        onClick={async () => { await forceSync(); await loadMeetings() }}
        disabled={isSyncing}
        className="text-sm text-white/80 font-medium disabled:opacity-50"
      >
        {isSyncing ? 'Syncing...' : 'Refresh'}
      </button>
    ),
  })
```

Note: `loadMeetings` is defined on line 17 but `useHeader` is called before it. We need to move the `useHeader` call after the `loadMeetings` definition. Place the `useHeader` call after line 26 (after the `loadMeetings` useCallback closing).

- [ ] **Step 3: Remove the old heading block**

Remove lines 48-61 (the `{/* Header */}` section with `<h1>` and refresh button):

```tsx
      {/* Header */}
      <div className="px-4 pt-4 pb-2">
        <div className="flex items-center justify-between">
          <h1 className="text-2xl font-bold text-iq-dark">Meetings</h1>
          <button
            onClick={handleRefresh}
            disabled={isSyncing}
            className="text-sm text-iq-blue font-medium disabled:opacity-50"
          >
            {isSyncing ? 'Syncing...' : 'Refresh'}
          </button>
        </div>
      </div>
```

Also remove the now-unused `handleRefresh` function (lines 32-35) since the refresh logic is inlined in the `useHeader` rightContent.

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 5: Commit**

```bash
git add mobile/src/components/MeetingsList.tsx
git commit -m "feat: migrate MeetingsList heading to HeaderContext"
```

---

### Task 4: Migrate SettingsScreen to useHeader

**Files:**
- Modify: `mobile/src/components/SettingsScreen.tsx:1-10` (imports), `108-109` (remove heading)

- [ ] **Step 1: Add useHeader import**

In `mobile/src/components/SettingsScreen.tsx`, add after line 10:

```tsx
import { useHeader } from '@/contexts/HeaderContext'
```

- [ ] **Step 2: Add useHeader call inside the component**

Inside `SettingsScreen`, after line 46 (`const [error, setError] = useState<string | null>(null)`), add:

```tsx
  useHeader({ title: 'Settings' })
```

- [ ] **Step 3: Remove the old heading**

Remove line 109:

```tsx
      <h1 className="text-2xl font-bold text-iq-dark mb-6">Settings</h1>
```

Add `mt-4` to the error div (previously line 111) to preserve spacing, changing `className="mb-4 p-3 ...` to `className="mb-4 mt-4 p-3 ...`. Alternatively, since the `<div className="px-4 pt-4 pb-24">` wrapper already has `pt-4`, the spacing should be fine without the `<h1>`'s `mb-6`. Verify visually.

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 5: Commit**

```bash
git add mobile/src/components/SettingsScreen.tsx
git commit -m "feat: migrate SettingsScreen heading to HeaderContext"
```

---

### Task 5: Migrate MeetingDetail to useHeader

**Files:**
- Modify: `mobile/src/components/MeetingDetail.tsx:1-11` (imports), `84-95` (remove header block)

- [ ] **Step 1: Add useHeader import**

In `mobile/src/components/MeetingDetail.tsx`, add after line 10:

```tsx
import { useHeader } from '@/contexts/HeaderContext'
```

- [ ] **Step 2: Remove the ArrowLeft import**

On line 11, remove `ArrowLeft` from the lucide-react import since the back button is now handled by AppHeader:

Change:
```tsx
import { ArrowLeft, FileText, BookOpen } from 'lucide-react'
```
To:
```tsx
import { FileText, BookOpen } from 'lucide-react'
```

- [ ] **Step 3: Add useHeader call inside the component**

Inside `MeetingDetail`, after line 21 (`const [loading, setLoading] = useState(true)`), add:

```tsx
  useHeader({
    title: meeting?.title || 'Loading...',
    showBack: true,
  })
```

This will update automatically when `meeting` state changes (loading → loaded).

- [ ] **Step 4: Remove the old header block**

Remove lines 86-95 (the back button and title inside the content area):

```tsx
      {/* Header */}
      <div className="px-4 pt-4 pb-2 border-b border-iq-light-shade">
        <div className="flex items-center gap-3 mb-2">
          <button onClick={() => router.back()} className="p-1 -ml-1">
            <ArrowLeft className="w-5 h-5 text-iq-medium" />
          </button>
          <h1 className="text-lg font-semibold text-iq-dark truncate flex-1">
            {meeting.title || 'Untitled Meeting'}
          </h1>
        </div>
```

Keep the status indicator (lines 97-105) and tabs (lines 107-132), but move them into a new wrapper div with the border-bottom styling. The result for lines 84-132 should be:

```tsx
  return (
    <div className="flex flex-col h-full">
      {/* Status + Tabs */}
      <div className="px-4 pt-2 pb-0 border-b border-iq-light-shade">
        {/* Status indicator for pending operations */}
        {meeting.status !== 'completed' && (
          <div className="text-xs text-iq-blue mb-2">
            {meeting.status === 'pending_upload' && 'Waiting to upload audio...'}
            {meeting.status === 'uploading' && 'Uploading audio...'}
            {meeting.status === 'transcribing' && 'Transcription in progress...'}
            {meeting.status === 'summarizing' && 'Generating summary...'}
          </div>
        )}

        {/* Tabs */}
        <div className="flex gap-4">
          <button
            onClick={() => setActiveTab('transcript')}
            className={`flex items-center gap-1.5 pb-2 text-sm font-medium border-b-2 ${
              activeTab === 'transcript'
                ? 'border-iq-blue text-iq-blue'
                : 'border-transparent text-iq-medium'
            }`}
          >
            <FileText className="w-4 h-4" />
            Transcript
          </button>
          <button
            onClick={() => setActiveTab('summary')}
            className={`flex items-center gap-1.5 pb-2 text-sm font-medium border-b-2 ${
              activeTab === 'summary'
                ? 'border-iq-blue text-iq-blue'
                : 'border-transparent text-iq-medium'
            }`}
          >
            <BookOpen className="w-4 h-4" />
            Summary
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {activeTab === 'transcript' ? (
          <TranscriptView
            text={meeting.transcript_text}
            segments={meeting.transcript_segments}
          />
        ) : (
          <SummaryView
            summary={meeting.summary}
            meetingId={meetingId}
            status={meeting.status}
            onGenerateSummary={handleGenerateSummary}
            isGenerating={isStarting || isSummaryPolling}
          />
        )}
      </div>
    </div>
  )
```

- [ ] **Step 5: Remove unused router import (if no longer used elsewhere)**

Check if `router` is still used. It is — in the `!meeting` return (line 77: `router.back()`). Keep the import but remove the `useRouter` usage only if that "Go back" button should also use AppHeader's back. Since that's a fallback state (meeting not found), keep `router.back()` there for now.

- [ ] **Step 6: Verify it compiles**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 7: Commit**

```bash
git add mobile/src/components/MeetingDetail.tsx
git commit -m "feat: migrate MeetingDetail heading and back button to HeaderContext"
```

---

### Task 6: Migrate RecordingScreen to useHeader

**Files:**
- Modify: `mobile/src/components/RecordingScreen.tsx:1-7` (imports), after line 14

- [ ] **Step 1: Add useHeader import**

In `mobile/src/components/RecordingScreen.tsx`, add after line 6:

```tsx
import { useHeader } from '@/contexts/HeaderContext'
```

- [ ] **Step 2: Add useHeader call inside the component**

Inside `RecordingScreen`, after line 14 (`const [error, setError] = useState<string | null>(null)`), add:

```tsx
  useHeader({ title: 'Record' })
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 4: Commit**

```bash
git add mobile/src/components/RecordingScreen.tsx
git commit -m "feat: migrate RecordingScreen heading to HeaderContext"
```

---

### Task 7: Run all tests and verify

**Files:** No changes — verification only

- [ ] **Step 1: Run all existing tests**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx vitest run`
Expected: All tests pass (existing context tests + new HeaderContext tests)

- [ ] **Step 2: Run type check**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 3: Verify no orphaned heading elements**

Run: `cd /Users/davidlynes/Documents/meeting-notes/mobile && grep -rn '<h1' src/components/MeetingsList.tsx src/components/SettingsScreen.tsx src/components/MeetingDetail.tsx src/components/RecordingScreen.tsx`
Expected: No matches (all `<h1>` headings removed from these files)

- [ ] **Step 4: Final commit if any fixes were needed**

Only if fixes were required in steps 1-3.

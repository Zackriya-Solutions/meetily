import { describe, it, expect, vi } from 'vitest'
import { render, renderHook, act } from '@testing-library/react'
import React, { useState } from 'react'
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
    // Need a component tree where setter unmounts but reader stays mounted
    let capturedTitle = ''
    let toggleSetter: () => void

    function Setter() {
      useHeader({ title: 'Settings' })
      return null
    }

    function Reader() {
      const { title } = useHeaderContext()
      capturedTitle = title
      return null
    }

    function TestApp() {
      const [showSetter, setShowSetter] = useState(true)
      toggleSetter = () => setShowSetter(false)
      return (
        <HeaderProvider>
          {showSetter && <Setter />}
          <Reader />
        </HeaderProvider>
      )
    }

    render(<TestApp />)
    expect(capturedTitle).toBe('Settings')

    act(() => { toggleSetter() })
    expect(capturedTitle).toBe('IQ:capture')
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

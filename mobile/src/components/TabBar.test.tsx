import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import TabBar from './TabBar'

// We need to override the default usePathname mock per-test
const mockUsePathname = vi.fn(() => '/')

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => mockUsePathname(),
  useSearchParams: () => new URLSearchParams(),
}))

describe('TabBar', () => {
  beforeEach(() => {
    mockUsePathname.mockReturnValue('/')
  })

  it('renders 3 tabs', () => {
    render(<TabBar />)
    expect(screen.getByText('Meetings')).toBeInTheDocument()
    expect(screen.getByText('Record')).toBeInTheDocument()
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders correct link hrefs', () => {
    render(<TabBar />)
    const meetingsLink = screen.getByText('Meetings').closest('a')
    const recordLink = screen.getByText('Record').closest('a')
    const settingsLink = screen.getByText('Settings').closest('a')

    expect(meetingsLink).toHaveAttribute('href', '/')
    expect(recordLink).toHaveAttribute('href', '/record')
    expect(settingsLink).toHaveAttribute('href', '/settings')
  })

  it('highlights Meetings tab when pathname is /', () => {
    mockUsePathname.mockReturnValue('/')
    render(<TabBar />)

    const meetingsLink = screen.getByText('Meetings').closest('a')
    const recordLink = screen.getByText('Record').closest('a')

    expect(meetingsLink?.className).toContain('text-blue-600')
    expect(recordLink?.className).toContain('text-gray-500')
  })

  it('highlights Record tab when pathname is /record', () => {
    mockUsePathname.mockReturnValue('/record')
    render(<TabBar />)

    const meetingsLink = screen.getByText('Meetings').closest('a')
    const recordLink = screen.getByText('Record').closest('a')

    expect(meetingsLink?.className).toContain('text-gray-500')
    expect(recordLink?.className).toContain('text-blue-600')
  })

  it('highlights Settings tab when pathname is /settings', () => {
    mockUsePathname.mockReturnValue('/settings')
    render(<TabBar />)

    const settingsLink = screen.getByText('Settings').closest('a')
    expect(settingsLink?.className).toContain('text-blue-600')
  })

  it('highlights Settings tab for nested settings paths', () => {
    mockUsePathname.mockReturnValue('/settings/profile')
    render(<TabBar />)

    const settingsLink = screen.getByText('Settings').closest('a')
    expect(settingsLink?.className).toContain('text-blue-600')
  })

  it('does not highlight Meetings tab for non-root paths', () => {
    mockUsePathname.mockReturnValue('/record')
    render(<TabBar />)

    const meetingsLink = screen.getByText('Meetings').closest('a')
    expect(meetingsLink?.className).toContain('text-gray-500')
  })

  it('renders as a nav element', () => {
    render(<TabBar />)
    expect(document.querySelector('nav')).toBeInTheDocument()
  })
})

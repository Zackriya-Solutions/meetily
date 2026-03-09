import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import TranscriptView from './TranscriptView'
import { TranscriptSegment } from '@/types'

describe('TranscriptView', () => {
  it('shows empty state when no text and no segments', () => {
    render(<TranscriptView />)
    expect(screen.getByText('No transcript available yet.')).toBeInTheDocument()
    expect(
      screen.getByText('Transcript will appear here once audio is uploaded and processed.')
    ).toBeInTheDocument()
  })

  it('shows empty state when text is undefined and segments is empty array', () => {
    render(<TranscriptView text={undefined} segments={[]} />)
    expect(screen.getByText('No transcript available yet.')).toBeInTheDocument()
  })

  it('renders raw text when only text is provided', () => {
    render(<TranscriptView text="Hello, this is a transcript." />)
    expect(screen.getByText('Hello, this is a transcript.')).toBeInTheDocument()
  })

  it('renders segments with timestamps when segments are provided', () => {
    const segments: TranscriptSegment[] = [
      { text: 'Welcome everyone.', start: 0, end: 3, confidence: 0.95 },
      { text: 'Let us begin.', start: 3, end: 6, confidence: 0.92 },
    ]
    render(<TranscriptView segments={segments} />)
    expect(screen.getByText('Welcome everyone.')).toBeInTheDocument()
    expect(screen.getByText('Let us begin.')).toBeInTheDocument()
    expect(screen.getByText('00:00')).toBeInTheDocument()
    expect(screen.getByText('00:03')).toBeInTheDocument()
  })

  it('formats timestamps correctly for minutes', () => {
    const segments: TranscriptSegment[] = [
      { text: 'Later in the meeting.', start: 125, end: 130, confidence: 0.9 },
    ]
    render(<TranscriptView segments={segments} />)
    expect(screen.getByText('02:05')).toBeInTheDocument()
  })

  it('prefers segments over text when both are provided', () => {
    const segments: TranscriptSegment[] = [
      { text: 'Segment text.', start: 0, end: 5, confidence: 0.9 },
    ]
    render(<TranscriptView text="Raw text" segments={segments} />)
    expect(screen.getByText('Segment text.')).toBeInTheDocument()
    // Raw text should NOT be rendered as a standalone block
    // (it won't be in its own <p> with whitespace-pre-wrap)
    expect(screen.queryByText('Raw text')).not.toBeInTheDocument()
  })

  it('renders text as fallback when segments array is empty', () => {
    render(<TranscriptView text="Fallback transcript" segments={[]} />)
    expect(screen.getByText('Fallback transcript')).toBeInTheDocument()
  })

  it('preserves whitespace in raw text mode', () => {
    render(<TranscriptView text={'Line 1\nLine 2'} />)
    const textEl = screen.getByText((_, el) =>
      el?.tagName === 'P' && el?.textContent === 'Line 1\nLine 2'
    )
    expect(textEl.className).toContain('whitespace-pre-wrap')
  })
})

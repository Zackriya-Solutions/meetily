import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import SummaryView from './SummaryView'
import { SummaryDataResponse } from '@/types'

describe('SummaryView', () => {
  it('shows generating spinner when status is summarizing', () => {
    const { container } = render(
      <SummaryView meetingId="m1" status="summarizing" />
    )
    expect(screen.getByText('Generating summary...')).toBeInTheDocument()
    expect(container.querySelector('.animate-spin')).toBeInTheDocument()
  })

  it('shows generating spinner when isGenerating is true', () => {
    render(
      <SummaryView meetingId="m1" status="completed" isGenerating={true} />
    )
    expect(screen.getByText('Generating summary...')).toBeInTheDocument()
  })

  it('shows "no summary" message when summary is undefined and status is completed', () => {
    render(
      <SummaryView meetingId="m1" status="completed" />
    )
    expect(screen.getByText('No summary generated yet.')).toBeInTheDocument()
  })

  it('shows "Generate Summary" button when status is completed and onGenerateSummary is provided', () => {
    const mockGenerate = vi.fn()
    render(
      <SummaryView
        meetingId="m1"
        status="completed"
        onGenerateSummary={mockGenerate}
      />
    )
    expect(screen.getByText('Generate Summary')).toBeInTheDocument()
  })

  it('does not show Generate Summary button when status is not completed', () => {
    render(
      <SummaryView
        meetingId="m1"
        status="transcribing"
        onGenerateSummary={vi.fn()}
      />
    )
    expect(screen.queryByText('Generate Summary')).not.toBeInTheDocument()
  })

  it('shows waiting message when status is not completed and no summary', () => {
    render(
      <SummaryView meetingId="m1" status="transcribing" />
    )
    expect(
      screen.getByText('Summary will be available after transcription completes.')
    ).toBeInTheDocument()
  })

  it('calls onGenerateSummary when Generate Summary button is clicked', async () => {
    const user = userEvent.setup()
    const mockGenerate = vi.fn()
    render(
      <SummaryView
        meetingId="m1"
        status="completed"
        onGenerateSummary={mockGenerate}
      />
    )
    await user.click(screen.getByText('Generate Summary'))
    expect(mockGenerate).toHaveBeenCalledTimes(1)
  })

  it('does not show Generate Summary button when onGenerateSummary is not provided', () => {
    render(
      <SummaryView meetingId="m1" status="completed" />
    )
    expect(screen.queryByText('Generate Summary')).not.toBeInTheDocument()
  })

  it('renders summary sections in order', () => {
    const summary: SummaryDataResponse = {
      _section_order: ['overview', 'action_items'],
      overview: {
        title: 'Overview',
        blocks: [
          { id: 'b1', type: 'text', content: 'The team discussed Q3 goals.', color: '' },
        ],
      },
      action_items: {
        title: 'Action Items',
        blocks: [
          { id: 'b2', type: 'text', content: 'Follow up with client.', color: '' },
          { id: 'b3', type: 'text', content: 'Prepare slides.', color: '' },
        ],
      },
    }
    render(
      <SummaryView meetingId="m1" status="completed" summary={summary} />
    )
    expect(screen.getByText('Overview')).toBeInTheDocument()
    expect(screen.getByText('Action Items')).toBeInTheDocument()
    expect(screen.getByText('The team discussed Q3 goals.')).toBeInTheDocument()
    expect(screen.getByText('Follow up with client.')).toBeInTheDocument()
    expect(screen.getByText('Prepare slides.')).toBeInTheDocument()
  })

  it('renders sections in the order specified by _section_order', () => {
    const summary: SummaryDataResponse = {
      _section_order: ['decisions', 'overview'],
      decisions: {
        title: 'Decisions',
        blocks: [{ id: 'd1', type: 'text', content: 'Decision content', color: '' }],
      },
      overview: {
        title: 'Overview',
        blocks: [{ id: 'o1', type: 'text', content: 'Overview content', color: '' }],
      },
    }
    const { container } = render(
      <SummaryView meetingId="m1" status="completed" summary={summary} />
    )
    const headings = container.querySelectorAll('h3')
    expect(headings[0]?.textContent).toBe('Decisions')
    expect(headings[1]?.textContent).toBe('Overview')
  })

  it('skips sections without a title', () => {
    const summary: SummaryDataResponse = {
      _section_order: ['good', 'bad'],
      good: {
        title: 'Good Section',
        blocks: [{ id: 'g1', type: 'text', content: 'Content', color: '' }],
      },
      bad: { title: '', blocks: [] },
    }
    render(
      <SummaryView meetingId="m1" status="completed" summary={summary} />
    )
    expect(screen.getByText('Good Section')).toBeInTheDocument()
    const headings = document.querySelectorAll('h3')
    expect(headings.length).toBe(1)
  })

  it('shows no summary state when _section_order is empty', () => {
    const summary: SummaryDataResponse = { _section_order: [] }
    render(
      <SummaryView meetingId="m1" status="completed" summary={summary} />
    )
    expect(screen.getByText('No summary generated yet.')).toBeInTheDocument()
  })

  it('handles missing _section_order gracefully', () => {
    const summary: SummaryDataResponse = {}
    render(
      <SummaryView meetingId="m1" status="completed" summary={summary} />
    )
    expect(screen.getByText('No summary generated yet.')).toBeInTheDocument()
  })
})

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { LanguageSelection } from './LanguageSelection';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@/lib/analytics', () => ({
  default: {
    track: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock('@/contexts/ConfigContext', () => ({
  useConfig: vi.fn().mockReturnValue({
    setSelectedLanguage: vi.fn(),
  }),
}));

describe('LanguageSelection', () => {
  const defaultProps = {
    selectedLanguage: 'auto',
    onLanguageChange: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it('renders the "Transcription Language" heading', () => {
    render(<LanguageSelection {...defaultProps} />);
    expect(screen.getByText('Transcription Language')).toBeInTheDocument();
  });

  it('renders a select dropdown', () => {
    render(<LanguageSelection {...defaultProps} />);
    const select = screen.getByRole('combobox');
    expect(select).toBeInTheDocument();
  });

  it('shows "Auto Detect (Original Language)" as default option', () => {
    render(<LanguageSelection {...defaultProps} />);
    const options = screen.getAllByText('Auto Detect (Original Language)');
    expect(options.length).toBeGreaterThanOrEqual(1);
    // Verify the option element exists
    const optionElement = options.find(el => el.tagName === 'OPTION');
    expect(optionElement).toBeDefined();
  });

  it('shows the selected language name in info text', () => {
    render(<LanguageSelection {...defaultProps} selectedLanguage="en" />);
    // "Current: English" appears in the info text
    expect(screen.getByText(/Current:/)).toBeInTheDocument();
    expect(screen.getByText(/optimised for/)).toBeInTheDocument();
  });

  it('shows auto-detect warning when auto is selected', () => {
    render(<LanguageSelection {...defaultProps} selectedLanguage="auto" />);
    expect(screen.getByText(/Auto Detect may produce incorrect results/)).toBeInTheDocument();
  });

  it('shows translation mode info when auto-translate is selected', () => {
    render(<LanguageSelection {...defaultProps} selectedLanguage="auto-translate" />);
    expect(screen.getByText(/Translation Mode Active/)).toBeInTheDocument();
  });

  it('shows optimization info when specific language is selected', () => {
    render(<LanguageSelection {...defaultProps} selectedLanguage="fr" />);
    expect(screen.getByText(/optimised for/)).toBeInTheDocument();
    // "French" appears in both the option and the info text, so use getAllByText
    const frenchElements = screen.getAllByText(/French/);
    expect(frenchElements.length).toBeGreaterThanOrEqual(2); // option + info text
  });

  it('calls onLanguageChange and invoke when selection changes', async () => {
    const onLanguageChange = vi.fn();
    render(<LanguageSelection {...defaultProps} onLanguageChange={onLanguageChange} />);

    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'en' } });

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('set_language_preference', { language: 'en' });
      expect(onLanguageChange).toHaveBeenCalledWith('en');
    });
  });

  it('disables the select when disabled prop is true', () => {
    render(<LanguageSelection {...defaultProps} disabled={true} />);
    expect(screen.getByRole('combobox')).toBeDisabled();
  });

  it('includes language code in option text for non-auto languages', () => {
    render(<LanguageSelection {...defaultProps} />);
    // English should show as "English (en)"
    expect(screen.getByText('English (en)')).toBeInTheDocument();
    expect(screen.getByText('French (fr)')).toBeInTheDocument();
  });

  it('does not include language code for auto options', () => {
    render(<LanguageSelection {...defaultProps} />);
    // Auto options should not have code suffix - find the option element specifically
    const autoOptions = screen.getAllByText('Auto Detect (Original Language)');
    const optionElement = autoOptions.find(el => el.tagName === 'OPTION');
    expect(optionElement).toBeDefined();
    expect(optionElement!.textContent).not.toContain('(auto)');
  });

  it('shows Parakeet warning when provider is parakeet', () => {
    render(<LanguageSelection {...defaultProps} provider="parakeet" />);
    expect(screen.getByText(/Parakeet Language Support/)).toBeInTheDocument();
  });

  it('limits languages to auto options for parakeet provider', () => {
    render(<LanguageSelection {...defaultProps} provider="parakeet" />);
    const select = screen.getByRole('combobox');
    const options = select.querySelectorAll('option');
    // Only auto and auto-translate for parakeet
    expect(options.length).toBe(2);
  });

  it('shows all languages for localWhisper provider', () => {
    render(<LanguageSelection {...defaultProps} provider="localWhisper" />);
    const select = screen.getByRole('combobox');
    const options = select.querySelectorAll('option');
    // Should have many languages (auto + auto-translate + all specific languages)
    expect(options.length).toBeGreaterThan(50);
  });
});

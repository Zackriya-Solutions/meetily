import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { VerifyEmailForm } from './VerifyEmailForm';

const mockVerifyEmail = vi.fn();
const mockResendVerification = vi.fn();

vi.mock('@/services/authService', () => ({
  verifyEmail: (...args: any[]) => mockVerifyEmail(...args),
  resendVerification: (...args: any[]) => mockResendVerification(...args),
}));

describe('VerifyEmailForm', () => {
  const defaultProps = {
    email: 'user@test.com',
    onSuccess: vi.fn(),
    onSwitchToLogin: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockVerifyEmail.mockResolvedValue({ message: 'Verified' });
    mockResendVerification.mockResolvedValue({ message: 'Sent' });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  /** Set code input value using fireEvent (avoids slow char-by-char userEvent.type on filtered inputs) */
  function setCode(value: string) {
    fireEvent.change(screen.getByLabelText('Verification Code'), { target: { value } });
  }

  function submitForm() {
    fireEvent.submit(screen.getByLabelText('Verification Code').closest('form')!);
  }

  describe('rendering', () => {
    it('renders instructional text with the provided email', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByText(/user@test.com/)).toBeInTheDocument();
    });

    it('renders verification code input', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByLabelText('Verification Code')).toBeInTheDocument();
    });

    it('renders verify email button', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Verify Email' })).toBeInTheDocument();
    });

    it('renders back to sign in link', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByText('Back to Sign In')).toBeInTheDocument();
    });

    it('renders resend code link', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByText('Resend code')).toBeInTheDocument();
    });

    it('disables submit when code is not 6 digits', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Verify Email' })).toBeDisabled();
    });

    it('renders code input with text type', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByLabelText('Verification Code')).toHaveAttribute('type', 'text');
    });

    it('renders code input with maxLength 6', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      expect(screen.getByLabelText('Verification Code')).toHaveAttribute('maxLength', '6');
    });
  });

  describe('code input validation', () => {
    it('only allows numeric input', async () => {
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      const codeInput = screen.getByLabelText('Verification Code');
      await user.type(codeInput, 'abc123xyz456');
      expect(codeInput).toHaveValue('123456');
    });

    it('limits to 6 digits', async () => {
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      const codeInput = screen.getByLabelText('Verification Code');
      await user.type(codeInput, '12345678');
      expect(codeInput).toHaveValue('123456');
    });

    it('enables submit button when 6 digits are entered', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('123456');
      expect(screen.getByRole('button', { name: 'Verify Email' })).not.toBeDisabled();
    });

    it('keeps submit disabled with fewer than 6 digits', () => {
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('12345');
      expect(screen.getByRole('button', { name: 'Verify Email' })).toBeDisabled();
    });
  });

  describe('form submission', () => {
    it('calls verifyEmail with email and code on submit', async () => {
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('654321');
      submitForm();

      await waitFor(() => {
        expect(mockVerifyEmail).toHaveBeenCalledWith('user@test.com', '654321');
      });
    });

    it('shows success message after verification', async () => {
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('654321');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Email verified! You can now sign in.')).toBeInTheDocument();
      });
    });

    it('calls onSuccess after delay on successful verification', async () => {
      vi.useFakeTimers();
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('654321');
      submitForm();

      // Flush microtasks so the async handler completes under fake timers
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByText('Email verified! You can now sign in.')).toBeInTheDocument();

      await vi.advanceTimersByTimeAsync(1500);
      expect(defaultProps.onSuccess).toHaveBeenCalled();
    });

    it('shows loading state during submission', async () => {
      mockVerifyEmail.mockImplementation(() => new Promise(() => {}));
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('654321');
      submitForm();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Verifying...' })).toBeDisabled();
      });
    });

    it('shows error on verification failure', async () => {
      mockVerifyEmail.mockRejectedValue(new Error('Invalid code'));
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('000000');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Invalid code')).toBeInTheDocument();
      });
    });

    it('shows generic error for non-Error objects', async () => {
      mockVerifyEmail.mockRejectedValue('unknown');
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('000000');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Verification failed')).toBeInTheDocument();
      });
    });

    it('does not call onSuccess on failure', async () => {
      mockVerifyEmail.mockRejectedValue(new Error('Invalid code'));
      render(<VerifyEmailForm {...defaultProps} />);
      setCode('000000');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Invalid code')).toBeInTheDocument();
      });
      expect(defaultProps.onSuccess).not.toHaveBeenCalled();
    });
  });

  describe('resend verification', () => {
    it('calls resendVerification with email when resend is clicked', async () => {
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(mockResendVerification).toHaveBeenCalledWith('user@test.com');
      });
    });

    it('shows success message after resend', async () => {
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(screen.getByText('A new code has been sent.')).toBeInTheDocument();
      });
    });

    it('shows loading state while resending', async () => {
      mockResendVerification.mockImplementation(() => new Promise(() => {}));
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      expect(screen.getByText('Sending...')).toBeInTheDocument();
    });

    it('shows error on resend failure', async () => {
      mockResendVerification.mockRejectedValue(new Error('Too many requests'));
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(screen.getByText('Too many requests')).toBeInTheDocument();
      });
    });

    it('shows generic error for non-Error resend failures', async () => {
      mockResendVerification.mockRejectedValue('fail');
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(screen.getByText('Could not resend code')).toBeInTheDocument();
      });
    });
  });

  describe('navigation', () => {
    it('calls onSwitchToLogin when back link is clicked', async () => {
      const user = userEvent.setup();
      render(<VerifyEmailForm {...defaultProps} />);
      await user.click(screen.getByText('Back to Sign In'));
      expect(defaultProps.onSwitchToLogin).toHaveBeenCalled();
    });
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { ResetPasswordForm } from './ResetPasswordForm';

const mockResetPassword = vi.fn();
const mockForgotPassword = vi.fn();

vi.mock('@/services/authService', () => ({
  resetPassword: (...args: any[]) => mockResetPassword(...args),
  forgotPassword: (...args: any[]) => mockForgotPassword(...args),
}));

describe('ResetPasswordForm', () => {
  const defaultProps = {
    email: 'user@test.com',
    onSwitchToLogin: vi.fn(),
    onSuccess: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockResetPassword.mockResolvedValue({ message: 'Password reset' });
    mockForgotPassword.mockResolvedValue({ message: 'Code sent' });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  /** Fill form using fireEvent for speed */
  function fillForm(code: string, newPw: string, confirm: string) {
    fireEvent.change(screen.getByLabelText('Reset Code'), { target: { value: code } });
    fireEvent.change(screen.getByLabelText('New Password'), { target: { value: newPw } });
    fireEvent.change(screen.getByLabelText('Confirm Password'), { target: { value: confirm } });
  }

  function submitForm() {
    fireEvent.submit(screen.getByLabelText('Reset Code').closest('form')!);
  }

  describe('rendering', () => {
    it('renders instructional text with email', () => {
      render(<ResetPasswordForm {...defaultProps} />);
      expect(screen.getByText(/user@test.com/)).toBeInTheDocument();
    });

    it('renders code, new password, and confirm password fields', () => {
      render(<ResetPasswordForm {...defaultProps} />);
      expect(screen.getByLabelText('Reset Code')).toBeInTheDocument();
      expect(screen.getByLabelText('New Password')).toBeInTheDocument();
      expect(screen.getByLabelText('Confirm Password')).toBeInTheDocument();
    });

    it('renders reset password button', () => {
      render(<ResetPasswordForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Reset Password' })).toBeInTheDocument();
    });

    it('renders back to sign in and resend code links', () => {
      render(<ResetPasswordForm {...defaultProps} />);
      expect(screen.getByText('Back to Sign In')).toBeInTheDocument();
      expect(screen.getByText('Resend code')).toBeInTheDocument();
    });

    it('disables submit when code is not 6 digits', () => {
      render(<ResetPasswordForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Reset Password' })).toBeDisabled();
    });
  });

  describe('form validation', () => {
    it('shows error when passwords do not match', async () => {
      render(<ResetPasswordForm {...defaultProps} />);
      fillForm('123456', 'Password1', 'Password2');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Passwords do not match')).toBeInTheDocument();
      });
      expect(mockResetPassword).not.toHaveBeenCalled();
    });

    it('only allows numeric input in code field', async () => {
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      const codeInput = screen.getByLabelText('Reset Code');
      await user.type(codeInput, 'abc123def456');
      expect(codeInput).toHaveValue('123456');
    });
  });

  describe('form submission', () => {
    it('calls resetPassword with correct arguments on submit', async () => {
      render(<ResetPasswordForm {...defaultProps} />);
      fillForm('123456', 'NewPass123', 'NewPass123');
      submitForm();

      await waitFor(() => {
        expect(mockResetPassword).toHaveBeenCalledWith('user@test.com', '123456', 'NewPass123');
      });
    });

    it('shows success message and calls onSuccess after delay', async () => {
      vi.useFakeTimers();
      render(<ResetPasswordForm {...defaultProps} />);
      fillForm('123456', 'NewPass123', 'NewPass123');
      submitForm();

      // Flush microtasks so the async handler completes under fake timers
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByText('Password reset successfully. You can now sign in.')).toBeInTheDocument();

      await vi.advanceTimersByTimeAsync(1500);
      expect(defaultProps.onSuccess).toHaveBeenCalled();
    });

    it('shows loading state during submission', async () => {
      mockResetPassword.mockImplementation(() => new Promise(() => {}));
      render(<ResetPasswordForm {...defaultProps} />);
      fillForm('123456', 'NewPass123', 'NewPass123');
      submitForm();

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Resetting...' })).toBeDisabled();
      });
    });

    it('shows error on failure', async () => {
      mockResetPassword.mockRejectedValue(new Error('Invalid code'));
      render(<ResetPasswordForm {...defaultProps} />);
      fillForm('123456', 'NewPass123', 'NewPass123');
      submitForm();

      await waitFor(() => {
        expect(screen.getByText('Invalid code')).toBeInTheDocument();
      });
    });
  });

  describe('resend code', () => {
    it('calls forgotPassword when resend is clicked', async () => {
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(mockForgotPassword).toHaveBeenCalledWith('user@test.com');
      });
    });

    it('shows success message after resend', async () => {
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(screen.getByText('A new code has been sent.')).toBeInTheDocument();
      });
    });

    it('shows loading state while resending', async () => {
      mockForgotPassword.mockImplementation(() => new Promise(() => {}));
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      expect(screen.getByText('Sending...')).toBeInTheDocument();
    });

    it('shows error on resend failure', async () => {
      mockForgotPassword.mockRejectedValue(new Error('Rate limited'));
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Resend code'));

      await waitFor(() => {
        expect(screen.getByText('Rate limited')).toBeInTheDocument();
      });
    });
  });

  describe('navigation', () => {
    it('calls onSwitchToLogin when back link is clicked', async () => {
      const user = userEvent.setup();
      render(<ResetPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Back to Sign In'));
      expect(defaultProps.onSwitchToLogin).toHaveBeenCalled();
    });
  });
});

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { ForgotPasswordForm } from './ForgotPasswordForm';

const mockForgotPassword = vi.fn();

vi.mock('@/services/authService', () => ({
  forgotPassword: (...args: any[]) => mockForgotPassword(...args),
}));

describe('ForgotPasswordForm', () => {
  const defaultProps = {
    onSwitchToLogin: vi.fn(),
    onCodeSent: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockForgotPassword.mockResolvedValue({ message: 'Code sent' });
  });

  describe('rendering', () => {
    it('renders instructional text', () => {
      render(<ForgotPasswordForm {...defaultProps} />);
      expect(screen.getByText(/Enter your email address/)).toBeInTheDocument();
    });

    it('renders email input', () => {
      render(<ForgotPasswordForm {...defaultProps} />);
      expect(screen.getByLabelText('Email')).toBeInTheDocument();
    });

    it('renders send reset code button', () => {
      render(<ForgotPasswordForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Send Reset Code' })).toBeInTheDocument();
    });

    it('renders back to sign in link', () => {
      render(<ForgotPasswordForm {...defaultProps} />);
      expect(screen.getByText('Back to Sign In')).toBeInTheDocument();
    });
  });

  describe('form submission', () => {
    it('calls forgotPassword with the entered email', async () => {
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      await waitFor(() => {
        expect(mockForgotPassword).toHaveBeenCalledWith('user@test.com');
      });
    });

    it('calls onCodeSent with email on success', async () => {
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      await waitFor(() => {
        expect(defaultProps.onCodeSent).toHaveBeenCalledWith('user@test.com');
      });
    });

    it('shows loading state during submission', async () => {
      mockForgotPassword.mockImplementation(() => new Promise(() => {}));
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      expect(screen.getByRole('button', { name: 'Sending...' })).toBeDisabled();
    });

    it('shows error message on failure', async () => {
      mockForgotPassword.mockRejectedValue(new Error('User not found'));
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      await waitFor(() => {
        expect(screen.getByText('User not found')).toBeInTheDocument();
      });
    });

    it('shows generic error for non-Error objects', async () => {
      mockForgotPassword.mockRejectedValue('unexpected');
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      await waitFor(() => {
        expect(screen.getByText('Request failed')).toBeInTheDocument();
      });
    });

    it('does not call onCodeSent on failure', async () => {
      mockForgotPassword.mockRejectedValue(new Error('Fail'));
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.click(screen.getByRole('button', { name: 'Send Reset Code' }));

      await waitFor(() => {
        expect(screen.getByText('Fail')).toBeInTheDocument();
      });
      expect(defaultProps.onCodeSent).not.toHaveBeenCalled();
    });
  });

  describe('navigation', () => {
    it('calls onSwitchToLogin when back link is clicked', async () => {
      const user = userEvent.setup();
      render(<ForgotPasswordForm {...defaultProps} />);
      await user.click(screen.getByText('Back to Sign In'));
      expect(defaultProps.onSwitchToLogin).toHaveBeenCalled();
    });
  });
});

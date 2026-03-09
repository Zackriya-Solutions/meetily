import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { RegisterForm } from './RegisterForm';

// Mock useAuth
const mockRegister = vi.fn();
const mockClearError = vi.fn();
let mockError: string | null = null;

vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => ({
    register: mockRegister,
    error: mockError,
    clearError: mockClearError,
  }),
}));

describe('RegisterForm', () => {
  const defaultProps = {
    deviceId: 'dev-abc',
    onSwitchToLogin: vi.fn(),
    onNeedsVerification: vi.fn(),
    onSuccess: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockError = null;
    mockRegister.mockResolvedValue(undefined);
  });

  describe('rendering', () => {
    it('renders display name, email, and password fields', () => {
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByLabelText(/Display Name/)).toBeInTheDocument();
      expect(screen.getByLabelText('Email')).toBeInTheDocument();
      expect(screen.getByLabelText('Password')).toBeInTheDocument();
    });

    it('renders the create account button', () => {
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Create Account' })).toBeInTheDocument();
    });

    it('renders sign in link', () => {
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByText('Sign in')).toBeInTheDocument();
    });

    it('shows display name as optional', () => {
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByText('(optional)')).toBeInTheDocument();
    });

    it('shows password requirements hint', () => {
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByText('Min 8 chars, 1 uppercase, 1 lowercase, 1 digit')).toBeInTheDocument();
    });
  });

  describe('form interactions', () => {
    it('allows typing in all fields', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      const nameInput = screen.getByLabelText(/Display Name/);
      const emailInput = screen.getByLabelText('Email');
      const passwordInput = screen.getByLabelText('Password');

      await user.type(nameInput, 'John Doe');
      await user.type(emailInput, 'john@example.com');
      await user.type(passwordInput, 'Pass1234');

      expect(nameInput).toHaveValue('John Doe');
      expect(emailInput).toHaveValue('john@example.com');
      expect(passwordInput).toHaveValue('Pass1234');
    });
  });

  describe('form submission', () => {
    it('calls register with all fields on submit', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText(/Display Name/), 'John Doe');
      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      await waitFor(() => {
        expect(mockRegister).toHaveBeenCalledWith('john@example.com', 'Pass1234', 'dev-abc', 'John Doe');
      });
    });

    it('passes undefined for display name when empty', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      await waitFor(() => {
        expect(mockRegister).toHaveBeenCalledWith('john@example.com', 'Pass1234', 'dev-abc', undefined);
      });
    });

    it('calls onNeedsVerification after successful registration', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      await waitFor(() => {
        expect(defaultProps.onNeedsVerification).toHaveBeenCalledWith('john@example.com');
      });
    });

    it('shows loading state during submission', async () => {
      mockRegister.mockImplementation(() => new Promise(() => {}));
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      expect(screen.getByRole('button', { name: 'Creating account...' })).toBeDisabled();
    });

    it('clears error before submission', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      expect(mockClearError).toHaveBeenCalled();
    });

    it('does not call onNeedsVerification on failure', async () => {
      mockRegister.mockRejectedValue(new Error('Email in use'));
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'john@example.com');
      await user.type(screen.getByLabelText('Password'), 'Pass1234');
      await user.click(screen.getByRole('button', { name: 'Create Account' }));

      await waitFor(() => {
        expect(mockRegister).toHaveBeenCalled();
      });
      expect(defaultProps.onNeedsVerification).not.toHaveBeenCalled();
    });
  });

  describe('error display', () => {
    it('displays error from auth context', () => {
      mockError = 'Email already exists';
      render(<RegisterForm {...defaultProps} />);
      expect(screen.getByText('Email already exists')).toBeInTheDocument();
    });

    it('does not display error when none exists', () => {
      mockError = null;
      render(<RegisterForm {...defaultProps} />);
      expect(screen.queryByText('Email already exists')).not.toBeInTheDocument();
    });
  });

  describe('navigation', () => {
    it('calls onSwitchToLogin when "Sign in" is clicked', async () => {
      const user = userEvent.setup();
      render(<RegisterForm {...defaultProps} />);
      await user.click(screen.getByText('Sign in'));
      expect(defaultProps.onSwitchToLogin).toHaveBeenCalled();
    });
  });
});

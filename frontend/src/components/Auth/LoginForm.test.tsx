import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { LoginForm } from './LoginForm';

// Mock useAuth
const mockLogin = vi.fn();
const mockClearError = vi.fn();
let mockError: string | null = null;

vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => ({
    login: mockLogin,
    error: mockError,
    clearError: mockClearError,
  }),
}));

describe('LoginForm', () => {
  const defaultProps = {
    deviceId: 'test-device-123',
    onSwitchToRegister: vi.fn(),
    onForgotPassword: vi.fn(),
    onNeedsVerification: vi.fn(),
    onSuccess: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockError = null;
    mockLogin.mockResolvedValue(undefined);
  });

  describe('rendering', () => {
    it('renders email and password fields', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByLabelText('Email')).toBeInTheDocument();
      expect(screen.getByLabelText('Password')).toBeInTheDocument();
    });

    it('renders the sign in button', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Sign In' })).toBeInTheDocument();
    });

    it('renders forgot password link', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByText('Forgot Password?')).toBeInTheDocument();
    });

    it('renders create account link', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByText('Create one')).toBeInTheDocument();
    });

    it('renders email input with correct type', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByLabelText('Email')).toHaveAttribute('type', 'email');
    });

    it('renders password input with correct type', () => {
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByLabelText('Password')).toHaveAttribute('type', 'password');
    });
  });

  describe('form interactions', () => {
    it('allows typing in email field', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);
      const emailInput = screen.getByLabelText('Email');
      await user.type(emailInput, 'test@example.com');
      expect(emailInput).toHaveValue('test@example.com');
    });

    it('allows typing in password field', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);
      const passwordInput = screen.getByLabelText('Password');
      await user.type(passwordInput, 'mypassword');
      expect(passwordInput).toHaveValue('mypassword');
    });
  });

  describe('form submission', () => {
    it('calls login with email, password, and deviceId on submit', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'password123');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      await waitFor(() => {
        expect(mockLogin).toHaveBeenCalledWith('user@test.com', 'password123', 'test-device-123');
      });
    });

    it('calls clearError before submission', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'password123');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      expect(mockClearError).toHaveBeenCalled();
    });

    it('calls onSuccess after successful login', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'password123');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      await waitFor(() => {
        expect(defaultProps.onSuccess).toHaveBeenCalled();
      });
    });

    it('shows loading state during submission', async () => {
      mockLogin.mockImplementation(() => new Promise(() => {})); // Never resolves
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'password123');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      expect(screen.getByRole('button', { name: 'Signing in...' })).toBeDisabled();
    });

    it('calls onNeedsVerification when error contains "not verified"', async () => {
      mockLogin.mockRejectedValue(new Error('Email not verified'));
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'password123');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      await waitFor(() => {
        expect(defaultProps.onNeedsVerification).toHaveBeenCalledWith('user@test.com');
      });
    });

    it('does not call onSuccess on login failure', async () => {
      mockLogin.mockRejectedValue(new Error('Bad credentials'));
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);

      await user.type(screen.getByLabelText('Email'), 'user@test.com');
      await user.type(screen.getByLabelText('Password'), 'wrong');
      await user.click(screen.getByRole('button', { name: 'Sign In' }));

      await waitFor(() => {
        expect(mockLogin).toHaveBeenCalled();
      });
      expect(defaultProps.onSuccess).not.toHaveBeenCalled();
    });
  });

  describe('error display', () => {
    it('displays error message from context', () => {
      mockError = 'Invalid credentials';
      render(<LoginForm {...defaultProps} />);
      expect(screen.getByText('Invalid credentials')).toBeInTheDocument();
    });

    it('does not display error when none exists', () => {
      mockError = null;
      render(<LoginForm {...defaultProps} />);
      expect(screen.queryByText('Invalid credentials')).not.toBeInTheDocument();
    });
  });

  describe('navigation callbacks', () => {
    it('calls onSwitchToRegister when "Create one" is clicked', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);
      await user.click(screen.getByText('Create one'));
      expect(defaultProps.onSwitchToRegister).toHaveBeenCalled();
    });

    it('calls onForgotPassword when "Forgot Password?" is clicked', async () => {
      const user = userEvent.setup();
      render(<LoginForm {...defaultProps} />);
      await user.click(screen.getByText('Forgot Password?'));
      expect(defaultProps.onForgotPassword).toHaveBeenCalled();
    });
  });
});

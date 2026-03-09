import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { AuthModal } from './AuthModal';

// Mock all child form components to isolate AuthModal behavior
vi.mock('./LoginForm', () => ({
  LoginForm: ({ onSwitchToRegister, onForgotPassword, onNeedsVerification, onSuccess, deviceId }: any) => (
    <div data-testid="login-form">
      <span data-testid="login-deviceId">{deviceId}</span>
      <button onClick={onSwitchToRegister}>GoToRegister</button>
      <button onClick={onForgotPassword}>GoToForgot</button>
      <button onClick={() => onNeedsVerification('verify@test.com')}>NeedsVerify</button>
      <button onClick={onSuccess}>LoginSuccess</button>
    </div>
  ),
}));

vi.mock('./RegisterForm', () => ({
  RegisterForm: ({ onSwitchToLogin, onNeedsVerification, onSuccess, deviceId }: any) => (
    <div data-testid="register-form">
      <span data-testid="register-deviceId">{deviceId}</span>
      <button onClick={onSwitchToLogin}>GoToLogin</button>
      <button onClick={() => onNeedsVerification('newuser@test.com')}>RegisterNeedsVerify</button>
      <button onClick={onSuccess}>RegisterSuccess</button>
    </div>
  ),
}));

vi.mock('./ForgotPasswordForm', () => ({
  ForgotPasswordForm: ({ onSwitchToLogin, onCodeSent }: any) => (
    <div data-testid="forgot-form">
      <button onClick={onSwitchToLogin}>BackToLogin</button>
      <button onClick={() => onCodeSent('reset@test.com')}>CodeSent</button>
    </div>
  ),
}));

vi.mock('./ResetPasswordForm', () => ({
  ResetPasswordForm: ({ email, onSwitchToLogin, onSuccess }: any) => (
    <div data-testid="reset-form">
      <span data-testid="reset-email">{email}</span>
      <button onClick={onSwitchToLogin}>BackToLoginFromReset</button>
      <button onClick={onSuccess}>ResetSuccess</button>
    </div>
  ),
}));

vi.mock('./VerifyEmailForm', () => ({
  VerifyEmailForm: ({ email, onSwitchToLogin, onSuccess }: any) => (
    <div data-testid="verify-form">
      <span data-testid="verify-email">{email}</span>
      <button onClick={onSwitchToLogin}>BackToLoginFromVerify</button>
      <button onClick={onSuccess}>VerifySuccess</button>
    </div>
  ),
}));

describe('AuthModal', () => {
  const defaultProps = {
    isOpen: true,
    onClose: vi.fn(),
    onSuccess: vi.fn(),
    deviceId: 'device-xyz',
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('visibility', () => {
    it('renders when isOpen is true', () => {
      render(<AuthModal {...defaultProps} />);
      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('does not render when isOpen is false', () => {
      render(<AuthModal {...defaultProps} isOpen={false} />);
      expect(screen.queryByTestId('login-form')).not.toBeInTheDocument();
    });
  });

  describe('initial state', () => {
    it('starts in login mode', () => {
      render(<AuthModal {...defaultProps} />);
      expect(screen.getByTestId('login-form')).toBeInTheDocument();
      expect(screen.queryByTestId('register-form')).not.toBeInTheDocument();
    });

    it('displays Sign In title', () => {
      render(<AuthModal {...defaultProps} />);
      expect(screen.getByText('Sign In')).toBeInTheDocument();
    });

    it('passes deviceId to LoginForm', () => {
      render(<AuthModal {...defaultProps} />);
      expect(screen.getByTestId('login-deviceId').textContent).toBe('device-xyz');
    });
  });

  describe('close button', () => {
    it('renders close button', () => {
      render(<AuthModal {...defaultProps} />);
      // The close button contains the x character
      expect(screen.getByText('\u00d7')).toBeInTheDocument();
    });

    it('calls onClose when close button is clicked', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('\u00d7'));
      expect(defaultProps.onClose).toHaveBeenCalled();
    });
  });

  describe('mode switching', () => {
    it('switches to register mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToRegister'));

      expect(screen.getByTestId('register-form')).toBeInTheDocument();
      expect(screen.queryByTestId('login-form')).not.toBeInTheDocument();
      expect(screen.getByText('Create Account')).toBeInTheDocument();
    });

    it('switches to forgot password mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToForgot'));

      expect(screen.getByTestId('forgot-form')).toBeInTheDocument();
      expect(screen.queryByTestId('login-form')).not.toBeInTheDocument();
      expect(screen.getByText('Forgot Password')).toBeInTheDocument();
    });

    it('switches to verify-email mode from login', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('NeedsVerify'));

      expect(screen.getByTestId('verify-form')).toBeInTheDocument();
      expect(screen.getByTestId('verify-email').textContent).toBe('verify@test.com');
      expect(screen.getByText('Verify Email')).toBeInTheDocument();
    });

    it('switches to verify-email mode from register', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      // First go to register
      await user.click(screen.getByText('GoToRegister'));
      // Then trigger verification
      await user.click(screen.getByText('RegisterNeedsVerify'));

      expect(screen.getByTestId('verify-form')).toBeInTheDocument();
      expect(screen.getByTestId('verify-email').textContent).toBe('newuser@test.com');
    });

    it('switches to reset-password mode from forgot-password', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      // Go to forgot password
      await user.click(screen.getByText('GoToForgot'));
      // Trigger code sent
      await user.click(screen.getByText('CodeSent'));

      expect(screen.getByTestId('reset-form')).toBeInTheDocument();
      expect(screen.getByTestId('reset-email').textContent).toBe('reset@test.com');
      expect(screen.getByText('Reset Password')).toBeInTheDocument();
    });

    it('switches back to login from register', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToRegister'));
      expect(screen.getByTestId('register-form')).toBeInTheDocument();

      await user.click(screen.getByText('GoToLogin'));
      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('switches back to login from forgot password', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToForgot'));
      await user.click(screen.getByText('BackToLogin'));

      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('switches back to login from reset password', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToForgot'));
      await user.click(screen.getByText('CodeSent'));
      await user.click(screen.getByText('BackToLoginFromReset'));

      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('switches back to login from verify email', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('NeedsVerify'));
      await user.click(screen.getByText('BackToLoginFromVerify'));

      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('returns to login after reset password success', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('GoToForgot'));
      await user.click(screen.getByText('CodeSent'));
      await user.click(screen.getByText('ResetSuccess'));

      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });

    it('returns to login after verify email success', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);

      await user.click(screen.getByText('NeedsVerify'));
      await user.click(screen.getByText('VerifySuccess'));

      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });
  });

  describe('re-open behavior', () => {
    it('resets to login mode when modal re-opens', async () => {
      const user = userEvent.setup();
      const { rerender } = render(<AuthModal {...defaultProps} />);

      // Navigate to register mode
      await user.click(screen.getByText('GoToRegister'));
      expect(screen.getByTestId('register-form')).toBeInTheDocument();

      // Close the modal
      rerender(<AuthModal {...defaultProps} isOpen={false} />);

      // Re-open the modal
      rerender(<AuthModal {...defaultProps} isOpen={true} />);

      // Should be back in login mode
      expect(screen.getByTestId('login-form')).toBeInTheDocument();
    });
  });

  describe('title display', () => {
    it('shows Sign In for login mode', () => {
      render(<AuthModal {...defaultProps} />);
      expect(screen.getByText('Sign In')).toBeInTheDocument();
    });

    it('shows Create Account for register mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('GoToRegister'));
      expect(screen.getByText('Create Account')).toBeInTheDocument();
    });

    it('shows Forgot Password for forgot-password mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('GoToForgot'));
      expect(screen.getByText('Forgot Password')).toBeInTheDocument();
    });

    it('shows Reset Password for reset-password mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('GoToForgot'));
      await user.click(screen.getByText('CodeSent'));
      expect(screen.getByText('Reset Password')).toBeInTheDocument();
    });

    it('shows Verify Email for verify-email mode', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('NeedsVerify'));
      expect(screen.getByText('Verify Email')).toBeInTheDocument();
    });
  });

  describe('onSuccess propagation', () => {
    it('calls onSuccess from login form', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('LoginSuccess'));
      expect(defaultProps.onSuccess).toHaveBeenCalled();
    });

    it('passes deviceId to register form', async () => {
      const user = userEvent.setup();
      render(<AuthModal {...defaultProps} />);
      await user.click(screen.getByText('GoToRegister'));
      expect(screen.getByTestId('register-deviceId').textContent).toBe('device-xyz');
    });
  });
});

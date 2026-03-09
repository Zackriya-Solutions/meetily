import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { ChangePasswordForm } from './ChangePasswordForm';

const mockChangePassword = vi.fn();

vi.mock('@/services/authService', () => ({
  changePassword: (...args: any[]) => mockChangePassword(...args),
}));

describe('ChangePasswordForm', () => {
  const defaultProps = {
    onClose: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockChangePassword.mockResolvedValue({ message: 'Changed' });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  /** Helper to fill the form using fireEvent (fast, no char-by-char typing) */
  function fillForm(current: string, newPw: string, confirm: string) {
    fireEvent.change(screen.getByLabelText('Current Password'), { target: { value: current } });
    fireEvent.change(screen.getByLabelText('New Password'), { target: { value: newPw } });
    fireEvent.change(screen.getByLabelText('Confirm New Password'), { target: { value: confirm } });
  }

  describe('rendering', () => {
    it('renders the modal with title', () => {
      render(<ChangePasswordForm {...defaultProps} />);
      expect(screen.getByRole('heading', { name: 'Change Password' })).toBeInTheDocument();
    });

    it('renders current password, new password, and confirm fields', () => {
      render(<ChangePasswordForm {...defaultProps} />);
      expect(screen.getByLabelText('Current Password')).toBeInTheDocument();
      expect(screen.getByLabelText('New Password')).toBeInTheDocument();
      expect(screen.getByLabelText('Confirm New Password')).toBeInTheDocument();
    });

    it('renders submit button', () => {
      render(<ChangePasswordForm {...defaultProps} />);
      expect(screen.getByRole('button', { name: 'Change Password' })).toBeInTheDocument();
    });

    it('renders close button', () => {
      render(<ChangePasswordForm {...defaultProps} />);
      expect(screen.getByText('\u00d7')).toBeInTheDocument();
    });

    it('shows password requirements hint', () => {
      render(<ChangePasswordForm {...defaultProps} />);
      expect(screen.getByText('Min 8 chars, 1 uppercase, 1 lowercase, 1 digit')).toBeInTheDocument();
    });
  });

  describe('form validation', () => {
    it('shows error when new passwords do not match', async () => {
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass1', 'NewPass1', 'NewPass2');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Passwords do not match')).toBeInTheDocument();
      });
      expect(mockChangePassword).not.toHaveBeenCalled();
    });
  });

  describe('form submission', () => {
    it('calls changePassword with current and new password', async () => {
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass123', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(mockChangePassword).toHaveBeenCalledWith('OldPass123', 'NewPass123');
      });
    });

    it('shows success message after change', async () => {
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass123', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Password changed successfully.')).toBeInTheDocument();
      });
    });

    it('calls onClose after successful change with delay', async () => {
      vi.useFakeTimers();
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass123', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      // Flush microtasks so the async handler completes under fake timers
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByText('Password changed successfully.')).toBeInTheDocument();

      await vi.advanceTimersByTimeAsync(1500);
      expect(defaultProps.onClose).toHaveBeenCalled();
    });

    it('shows loading state during submission', async () => {
      mockChangePassword.mockImplementation(() => new Promise(() => {}));
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass123', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Changing...' })).toBeDisabled();
      });
    });

    it('shows error on failure', async () => {
      mockChangePassword.mockRejectedValue(new Error('Wrong current password'));
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('WrongPass', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Wrong current password')).toBeInTheDocument();
      });
    });

    it('shows generic error for non-Error throws', async () => {
      mockChangePassword.mockRejectedValue('something');
      render(<ChangePasswordForm {...defaultProps} />);
      fillForm('OldPass123', 'NewPass123', 'NewPass123');
      fireEvent.submit(screen.getByLabelText('Current Password').closest('form')!);

      await waitFor(() => {
        expect(screen.getByText('Change failed')).toBeInTheDocument();
      });
    });
  });

  describe('close button', () => {
    it('calls onClose when close button is clicked', async () => {
      const user = userEvent.setup();
      render(<ChangePasswordForm {...defaultProps} />);
      await user.click(screen.getByText('\u00d7'));
      expect(defaultProps.onClose).toHaveBeenCalled();
    });
  });
});

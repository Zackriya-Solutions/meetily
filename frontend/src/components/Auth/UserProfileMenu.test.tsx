import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { UserProfileMenu } from './UserProfileMenu';

// Mock authService
const mockUpdateProfile = vi.fn();
const mockDeactivateAccount = vi.fn();
const mockDeleteAccount = vi.fn();

vi.mock('@/services/authService', () => ({
  updateProfile: (...args: any[]) => mockUpdateProfile(...args),
  deactivateAccount: (...args: any[]) => mockDeactivateAccount(...args),
  deleteAccount: (...args: any[]) => mockDeleteAccount(...args),
}));

// Mock ChangePasswordForm
vi.mock('./ChangePasswordForm', () => ({
  ChangePasswordForm: ({ onClose }: { onClose: () => void }) => (
    <div data-testid="change-password-form">
      <button onClick={onClose}>CloseChangePassword</button>
    </div>
  ),
}));

// Mock useAuth
const mockLogout = vi.fn();
let mockUser: any = null;
let mockIsAuthenticated = false;

vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => ({
    user: mockUser,
    logout: mockLogout,
    isAuthenticated: mockIsAuthenticated,
  }),
}));

const authenticatedUser = {
  user_id: 'u1',
  email: 'user@example.com',
  display_name: 'John Doe',
  account_level: 'pro',
  email_verified: true,
  devices: [
    { device_id: 'd1', linked_at: '2025-01-01', platform: 'windows', last_seen: '2025-01-02' },
    { device_id: 'd2', linked_at: '2025-02-01', platform: 'macos', last_seen: '2025-02-02' },
  ],
};

describe('UserProfileMenu', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUser = null;
    mockIsAuthenticated = false;
    mockLogout.mockResolvedValue(undefined);
    mockUpdateProfile.mockResolvedValue({ message: 'Updated' });
    mockDeactivateAccount.mockResolvedValue({ message: 'Deactivated' });
    mockDeleteAccount.mockResolvedValue({ message: 'Deleted' });
  });

  describe('visibility', () => {
    it('returns null when not authenticated', () => {
      mockIsAuthenticated = false;
      mockUser = null;
      const { container } = render(<UserProfileMenu />);
      expect(container.innerHTML).toBe('');
    });

    it('returns null when user is null even if authenticated flag is true', () => {
      mockIsAuthenticated = true;
      mockUser = null;
      const { container } = render(<UserProfileMenu />);
      expect(container.innerHTML).toBe('');
    });

    it('renders when authenticated with user', () => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
      render(<UserProfileMenu />);
      expect(screen.getByText('John Doe')).toBeInTheDocument();
    });
  });

  describe('user display', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('shows user display name', () => {
      render(<UserProfileMenu />);
      expect(screen.getByText('John Doe')).toBeInTheDocument();
    });

    it('shows user initial from display name', () => {
      render(<UserProfileMenu />);
      expect(screen.getByText('J')).toBeInTheDocument();
    });

    it('shows email when display name is null', () => {
      mockUser = { ...authenticatedUser, display_name: null };
      render(<UserProfileMenu />);
      expect(screen.getByText('user@example.com')).toBeInTheDocument();
    });

    it('shows first letter of email when display name is null', () => {
      mockUser = { ...authenticatedUser, display_name: null };
      render(<UserProfileMenu />);
      expect(screen.getByText('U')).toBeInTheDocument();
    });
  });

  describe('menu toggle', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('menu is closed by default', () => {
      render(<UserProfileMenu />);
      expect(screen.queryByText('Sign Out')).not.toBeInTheDocument();
    });

    it('opens menu on click', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('Sign Out')).toBeInTheDocument();
    });

    it('closes menu on second click', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);

      // The profile toggle button contains "John Doe" in a span
      const toggleButton = screen.getByRole('button', { name: /John Doe/ });

      // Open
      await user.click(toggleButton);
      expect(screen.getByText('Sign Out')).toBeInTheDocument();

      // Close
      await user.click(toggleButton);
      expect(screen.queryByText('Sign Out')).not.toBeInTheDocument();
    });
  });

  describe('menu content', () => {
    beforeEach(async () => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('shows user email in the menu', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('user@example.com')).toBeInTheDocument();
    });

    it('shows device count', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText(/2 devices linked/)).toBeInTheDocument();
    });

    it('shows singular device text for 1 device', async () => {
      mockUser = { ...authenticatedUser, devices: [authenticatedUser.devices[0]] };
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText(/1 device linked/)).toBeInTheDocument();
    });

    it('shows account level', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText(/pro/i)).toBeInTheDocument();
    });

    it('shows Edit button next to display name', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('Edit')).toBeInTheDocument();
    });

    it('shows Change Password option', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('Change Password')).toBeInTheDocument();
    });

    it('shows Sign Out option', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('Sign Out')).toBeInTheDocument();
    });

    it('shows Delete Account option', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      expect(screen.getByText('Delete Account')).toBeInTheDocument();
    });

    it('shows "User" when display name is null in the menu', async () => {
      mockUser = { ...authenticatedUser, display_name: null };
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('user@example.com'));
      // Inside the menu it shows "User" as fallback
      expect(screen.getByText('User')).toBeInTheDocument();
    });
  });

  describe('sign out', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('calls logout when Sign Out is clicked', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Sign Out'));

      await waitFor(() => {
        expect(mockLogout).toHaveBeenCalled();
      });
    });

    it('closes the menu before calling logout', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Sign Out'));

      // The menu should close
      await waitFor(() => {
        expect(screen.queryByText('Delete Account')).not.toBeInTheDocument();
      });
    });
  });

  describe('edit name', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('shows name editing input when Edit is clicked', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Edit'));

      expect(screen.getByPlaceholderText('Display name')).toBeInTheDocument();
    });

    it('pre-fills current display name in edit input', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Edit'));

      expect(screen.getByPlaceholderText('Display name')).toHaveValue('John Doe');
    });

    it('shows Save and Cancel buttons in edit mode', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Edit'));

      expect(screen.getByText('Save')).toBeInTheDocument();
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });

    it('cancels editing and returns to display mode', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Edit'));
      await user.click(screen.getByText('Cancel'));

      expect(screen.queryByPlaceholderText('Display name')).not.toBeInTheDocument();
      expect(screen.getByText('Edit')).toBeInTheDocument();
    });

    it('calls updateProfile when Save is clicked', async () => {
      // Mock window.location.reload
      const reloadMock = vi.fn();
      Object.defineProperty(window, 'location', {
        value: { reload: reloadMock },
        writable: true,
      });

      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Edit'));

      const input = screen.getByPlaceholderText('Display name');
      await user.clear(input);
      await user.type(input, 'Jane Doe');
      await user.click(screen.getByText('Save'));

      await waitFor(() => {
        expect(mockUpdateProfile).toHaveBeenCalledWith('Jane Doe');
      });
    });
  });

  describe('delete account', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('shows confirmation when Delete Account is clicked', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Delete Account'));

      expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
      expect(screen.getByText('Confirm Delete')).toBeInTheDocument();
    });

    it('cancels delete confirmation', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Delete Account'));
      await user.click(screen.getByText('Cancel'));

      expect(screen.queryByText('This cannot be undone.')).not.toBeInTheDocument();
      expect(screen.getByText('Delete Account')).toBeInTheDocument();
    });

    it('calls deleteAccount and logout on Confirm Delete', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Delete Account'));
      await user.click(screen.getByText('Confirm Delete'));

      await waitFor(() => {
        expect(mockDeleteAccount).toHaveBeenCalled();
      });
      await waitFor(() => {
        expect(mockLogout).toHaveBeenCalled();
      });
    });
  });

  describe('change password', () => {
    beforeEach(() => {
      mockIsAuthenticated = true;
      mockUser = authenticatedUser;
    });

    it('opens ChangePasswordForm when Change Password is clicked', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Change Password'));

      expect(screen.getByTestId('change-password-form')).toBeInTheDocument();
    });

    it('closes ChangePasswordForm via its onClose callback', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Change Password'));

      expect(screen.getByTestId('change-password-form')).toBeInTheDocument();

      await user.click(screen.getByText('CloseChangePassword'));

      expect(screen.queryByTestId('change-password-form')).not.toBeInTheDocument();
    });

    it('closes the menu when Change Password is clicked', async () => {
      const user = userEvent.setup();
      render(<UserProfileMenu />);
      await user.click(screen.getByText('John Doe'));
      await user.click(screen.getByText('Change Password'));

      // Menu should be closed (Sign Out not visible)
      expect(screen.queryByText('Sign Out')).not.toBeInTheDocument();
    });
  });
});

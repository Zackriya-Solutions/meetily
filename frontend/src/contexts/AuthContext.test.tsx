import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React from 'react';
import { AuthProvider, useAuth } from './AuthContext';

// Mock the authService module
const mockLogin = vi.fn();
const mockRegister = vi.fn();
const mockLogout = vi.fn();
const mockRefreshTokens = vi.fn();
const mockGetMe = vi.fn();
const mockGetAccessToken = vi.fn();
const mockInitCloudApiUrl = vi.fn();

vi.mock('@/services/authService', () => ({
  login: (...args: any[]) => mockLogin(...args),
  register: (...args: any[]) => mockRegister(...args),
  logout: (...args: any[]) => mockLogout(...args),
  refreshTokens: (...args: any[]) => mockRefreshTokens(...args),
  getMe: (...args: any[]) => mockGetMe(...args),
  getAccessToken: (...args: any[]) => mockGetAccessToken(...args),
  initCloudApiUrl: (...args: any[]) => mockInitCloudApiUrl(...args),
}));

const mockUser = {
  user_id: 'u1',
  email: 'test@example.com',
  display_name: 'Test User',
  account_level: 'free',
  email_verified: true,
  devices: [{ device_id: 'd1', linked_at: '2025-01-01', platform: 'windows', last_seen: '2025-01-02' }],
};

const mockAuthResponse = {
  access_token: 'at_123',
  refresh_token: 'rt_123',
  token_type: 'Bearer',
  user: mockUser,
};

// Helper component to consume the context
function TestConsumer() {
  const auth = useAuth();
  return (
    <div>
      <span data-testid="loading">{String(auth.isLoading)}</span>
      <span data-testid="authenticated">{String(auth.isAuthenticated)}</span>
      <span data-testid="error">{auth.error || 'none'}</span>
      <span data-testid="user">{auth.user ? auth.user.email : 'null'}</span>
      <button onClick={() => auth.login('a@b.com', 'pass1234', 'dev1').catch(() => {})}>Login</button>
      <button onClick={() => auth.register('a@b.com', 'pass1234', 'dev1', 'Name').catch(() => {})}>Register</button>
      <button onClick={() => auth.logout()}>Logout</button>
      <button onClick={() => auth.clearError()}>ClearError</button>
    </div>
  );
}

function renderWithProvider() {
  return render(
    <AuthProvider>
      <TestConsumer />
    </AuthProvider>
  );
}

describe('AuthContext', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default: no token, no session to restore
    mockInitCloudApiUrl.mockResolvedValue(undefined);
    mockGetAccessToken.mockResolvedValue(null);
    mockGetMe.mockResolvedValue(null);
    mockRefreshTokens.mockResolvedValue(null);
    mockLogout.mockResolvedValue(undefined);
  });

  describe('useAuth hook', () => {
    it('throws when used outside of AuthProvider', () => {
      const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
      expect(() => render(<TestConsumer />)).toThrow('useAuth must be used within AuthProvider');
      spy.mockRestore();
    });
  });

  describe('initial state and session restore', () => {
    it('starts with loading true and no user', () => {
      // Don't resolve getAccessToken yet so we can catch loading state
      mockGetAccessToken.mockReturnValue(new Promise(() => {}));
      renderWithProvider();
      expect(screen.getByTestId('loading').textContent).toBe('true');
      expect(screen.getByTestId('authenticated').textContent).toBe('false');
      expect(screen.getByTestId('user').textContent).toBe('null');
    });

    it('finishes loading with no user when no token exists', async () => {
      mockGetAccessToken.mockResolvedValue(null);
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('loading').textContent).toBe('false');
      });
      expect(screen.getByTestId('authenticated').textContent).toBe('false');
    });

    it('restores user session when token and profile exist', async () => {
      mockGetAccessToken.mockResolvedValue('token_abc');
      mockGetMe.mockResolvedValue(mockUser);
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('loading').textContent).toBe('false');
      });
      expect(screen.getByTestId('authenticated').textContent).toBe('true');
      expect(screen.getByTestId('user').textContent).toBe('test@example.com');
    });

    it('tries token refresh when getMe returns null', async () => {
      mockGetAccessToken.mockResolvedValue('expired_token');
      mockGetMe.mockResolvedValue(null);
      mockRefreshTokens.mockResolvedValue(mockAuthResponse);
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('loading').textContent).toBe('false');
      });
      expect(mockRefreshTokens).toHaveBeenCalled();
      expect(screen.getByTestId('authenticated').textContent).toBe('true');
    });

    it('clears user when token refresh also fails', async () => {
      mockGetAccessToken.mockResolvedValue('expired_token');
      mockGetMe.mockResolvedValue(null);
      mockRefreshTokens.mockResolvedValue(null);
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('loading').textContent).toBe('false');
      });
      expect(screen.getByTestId('authenticated').textContent).toBe('false');
    });

    it('handles session restore errors gracefully', async () => {
      mockGetAccessToken.mockRejectedValue(new Error('network'));
      const spy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      renderWithProvider();
      await waitFor(() => {
        expect(screen.getByTestId('loading').textContent).toBe('false');
      });
      expect(screen.getByTestId('authenticated').textContent).toBe('false');
      spy.mockRestore();
    });
  });

  describe('login', () => {
    it('calls apiLogin and sets user on success', async () => {
      const user = userEvent.setup();
      mockLogin.mockResolvedValue(mockAuthResponse);
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Login'));

      await waitFor(() => {
        expect(screen.getByTestId('authenticated').textContent).toBe('true');
      });
      expect(mockLogin).toHaveBeenCalledWith('a@b.com', 'pass1234', 'dev1');
    });

    it('sets error on login failure', async () => {
      const user = userEvent.setup();
      mockLogin.mockRejectedValue(new Error('Invalid credentials'));
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      // The login button click will throw, but the error is caught in the consumer
      await user.click(screen.getByText('Login'));

      await waitFor(() => {
        expect(screen.getByTestId('error').textContent).toBe('Invalid credentials');
      });
    });

    it('sets generic error when no message in error', async () => {
      const user = userEvent.setup();
      mockLogin.mockRejectedValue({});
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Login'));

      await waitFor(() => {
        expect(screen.getByTestId('error').textContent).toBe('Login failed');
      });
    });
  });

  describe('register', () => {
    it('calls apiRegister and sets user on success', async () => {
      const user = userEvent.setup();
      mockRegister.mockResolvedValue(mockAuthResponse);
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Register'));

      await waitFor(() => {
        expect(screen.getByTestId('authenticated').textContent).toBe('true');
      });
      expect(mockRegister).toHaveBeenCalledWith('a@b.com', 'pass1234', 'dev1', 'Name');
    });

    it('sets error on register failure', async () => {
      const user = userEvent.setup();
      mockRegister.mockRejectedValue(new Error('Email in use'));
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Register'));

      await waitFor(() => {
        expect(screen.getByTestId('error').textContent).toBe('Email in use');
      });
    });

    it('sets generic error when no message', async () => {
      const user = userEvent.setup();
      mockRegister.mockRejectedValue({});
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Register'));

      await waitFor(() => {
        expect(screen.getByTestId('error').textContent).toBe('Registration failed');
      });
    });
  });

  describe('logout', () => {
    it('clears user on logout', async () => {
      const user = userEvent.setup();
      // Start logged in
      mockGetAccessToken.mockResolvedValue('token');
      mockGetMe.mockResolvedValue(mockUser);
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('authenticated').textContent).toBe('true'));

      await user.click(screen.getByText('Logout'));

      await waitFor(() => {
        expect(screen.getByTestId('authenticated').textContent).toBe('false');
        expect(screen.getByTestId('user').textContent).toBe('null');
      });
      expect(mockLogout).toHaveBeenCalled();
    });
  });

  describe('clearError', () => {
    it('clears the error state', async () => {
      const user = userEvent.setup();
      mockLogin.mockRejectedValue(new Error('bad'));
      renderWithProvider();
      await waitFor(() => expect(screen.getByTestId('loading').textContent).toBe('false'));

      await user.click(screen.getByText('Login'));
      await waitFor(() => expect(screen.getByTestId('error').textContent).toBe('bad'));

      await user.click(screen.getByText('ClearError'));
      expect(screen.getByTestId('error').textContent).toBe('none');
    });
  });

  describe('token auto-refresh', () => {
    it('schedules token refresh after successful login', async () => {
      vi.useFakeTimers();
      mockLogin.mockResolvedValue(mockAuthResponse);
      mockRefreshTokens.mockResolvedValue(mockAuthResponse);

      renderWithProvider();
      // Flush initial session restore
      await act(async () => {
        await vi.advanceTimersByTimeAsync(100);
      });

      // Click login using fireEvent (compatible with fake timers)
      fireEvent.click(screen.getByText('Login'));

      // Flush login promise
      await act(async () => {
        await vi.advanceTimersByTimeAsync(100);
      });

      // Advance 12 minutes to trigger the scheduled refresh
      await act(async () => {
        await vi.advanceTimersByTimeAsync(12 * 60 * 1000);
      });

      expect(mockRefreshTokens).toHaveBeenCalled();
      vi.useRealTimers();
    });
  });
});

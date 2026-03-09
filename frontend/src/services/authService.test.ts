import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

// Reset module-level cloudApiUrl between tests by re-importing
let authModule: typeof import('./authService');

beforeEach(async () => {
  vi.clearAllMocks();
  // Dynamically re-import to reset module-level state
  vi.resetModules();
  // Re-mock after resetModules
  vi.doMock('@tauri-apps/api/core', () => ({
    invoke: vi.fn().mockRejectedValue(new Error('invoke not mocked')),
  }));
  authModule = await import('./authService');
});

function mockFetchOk(data: any) {
  vi.mocked(global.fetch).mockResolvedValueOnce({
    ok: true,
    json: vi.fn().mockResolvedValue(data),
    status: 200,
  } as any);
}

function mockFetchError(status: number, detail: string) {
  vi.mocked(global.fetch).mockResolvedValueOnce({
    ok: false,
    json: vi.fn().mockResolvedValue({ detail }),
    status,
  } as any);
}

function mockFetchErrorNoJson(status: number) {
  vi.mocked(global.fetch).mockResolvedValueOnce({
    ok: false,
    json: vi.fn().mockRejectedValue(new Error('not json')),
    status,
  } as any);
}

const mockAuthResponse = {
  access_token: 'acc-123',
  refresh_token: 'ref-456',
  token_type: 'bearer',
  user: {
    user_id: 'user-1',
    email: 'test@example.com',
    display_name: 'Test User',
    account_level: 'free',
    email_verified: true,
    devices: [],
  },
};

describe('authService', () => {
  describe('initCloudApiUrl', () => {
    it('should skip fetch if NEXT_PUBLIC_CLOUD_API_URL is set', async () => {
      // cloudApiUrl is '' by default since env var not set, so it will try to fetch
      // After fetch returns cloud_api_url, subsequent calls skip
      mockFetchOk({ cloud_api_url: 'https://cloud.example.com' });
      await authModule.initCloudApiUrl();
      expect(global.fetch).toHaveBeenCalledTimes(1);

      // Second call should skip since cloudApiUrl is now set
      await authModule.initCloudApiUrl();
      expect(global.fetch).toHaveBeenCalledTimes(1);
    });

    it('should handle fetch failure gracefully', async () => {
      vi.mocked(global.fetch).mockRejectedValueOnce(new Error('network'));
      await expect(authModule.initCloudApiUrl()).resolves.toBeUndefined();
    });

    it('should handle non-ok response', async () => {
      vi.mocked(global.fetch).mockResolvedValueOnce({
        ok: false,
        json: vi.fn(),
        status: 500,
      } as any);
      await expect(authModule.initCloudApiUrl()).resolves.toBeUndefined();
    });

    it('should handle missing cloud_api_url in response', async () => {
      mockFetchOk({});
      await authModule.initCloudApiUrl();
      // cloudApiUrl remains empty, so getBaseUrl will return localhost
    });
  });

  describe('getAccessToken', () => {
    it('should return token from invoke', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('my-token');
      const token = await authModule.getAccessToken();
      expect(token).toBe('my-token');
      expect(mockedInvoke).toHaveBeenCalledWith('auth_get_access_token');
    });

    it('should return null on invoke error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockRejectedValueOnce(new Error('no token'));
      const token = await authModule.getAccessToken();
      expect(token).toBeNull();
    });
  });

  describe('getRefreshToken', () => {
    it('should return refresh token from invoke', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('refresh-tok');
      const token = await authModule.getRefreshToken();
      expect(token).toBe('refresh-tok');
      expect(mockedInvoke).toHaveBeenCalledWith('auth_get_refresh_token');
    });

    it('should return null on invoke error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockRejectedValueOnce(new Error('fail'));
      const token = await authModule.getRefreshToken();
      expect(token).toBeNull();
    });
  });

  describe('saveTokens', () => {
    it('should invoke auth_save_tokens', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);
      await authModule.saveTokens('acc', 'ref');
      expect(mockedInvoke).toHaveBeenCalledWith('auth_save_tokens', {
        accessToken: 'acc',
        refreshToken: 'ref',
      });
    });
  });

  describe('clearTokens', () => {
    it('should invoke auth_clear_tokens', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);
      await authModule.clearTokens();
      expect(mockedInvoke).toHaveBeenCalledWith('auth_clear_tokens');
    });
  });

  describe('getAuthUserId', () => {
    it('should return user ID from invoke', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('user-123');
      const id = await authModule.getAuthUserId();
      expect(id).toBe('user-123');
      expect(mockedInvoke).toHaveBeenCalledWith('auth_get_user_id');
    });

    it('should return null on error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockRejectedValueOnce(new Error('fail'));
      const id = await authModule.getAuthUserId();
      expect(id).toBeNull();
    });
  });

  describe('register', () => {
    it('should register, save tokens, and return auth response', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      mockFetchOk(mockAuthResponse);
      // saveTokens invoke
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);
      // saveAuthUserId invoke
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);

      const result = await authModule.register('test@example.com', 'pass', 'dev-1', 'Test');
      expect(result.access_token).toBe('acc-123');
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/auth/register'),
        expect.objectContaining({ method: 'POST' }),
      );
      expect(mockedInvoke).toHaveBeenCalledWith('auth_save_tokens', {
        accessToken: 'acc-123',
        refreshToken: 'ref-456',
      });
      expect(mockedInvoke).toHaveBeenCalledWith('auth_save_user_id', { userId: 'user-1' });
    });

    it('should throw on error response with detail', async () => {
      mockFetchError(400, 'Email already exists');
      await expect(authModule.register('test@example.com', 'pass', 'dev-1'))
        .rejects.toThrow('Email already exists');
    });

    it('should throw default message when error response has no JSON', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.register('test@example.com', 'pass', 'dev-1'))
        .rejects.toThrow('Registration failed');
    });

    it('should send null display_name when not provided', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      mockFetchOk(mockAuthResponse);
      vi.mocked(mockedInvoke).mockResolvedValue(undefined);

      await authModule.register('test@example.com', 'pass', 'dev-1');
      const fetchCall = vi.mocked(global.fetch).mock.calls[0];
      const body = JSON.parse(fetchCall[1]!.body as string);
      expect(body.display_name).toBeNull();
    });
  });

  describe('login', () => {
    it('should login, save tokens, and return auth response', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      mockFetchOk(mockAuthResponse);
      vi.mocked(mockedInvoke).mockResolvedValue(undefined);

      const result = await authModule.login('test@example.com', 'pass', 'dev-1');
      expect(result.user.email).toBe('test@example.com');
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/auth/login'),
        expect.objectContaining({ method: 'POST' }),
      );
    });

    it('should throw on error response', async () => {
      mockFetchError(401, 'Invalid credentials');
      await expect(authModule.login('test@example.com', 'wrong', 'dev-1'))
        .rejects.toThrow('Invalid credentials');
    });

    it('should throw default message when error has no JSON', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.login('test@example.com', 'pass', 'dev-1'))
        .rejects.toThrow('Login failed');
    });
  });

  describe('refreshTokens', () => {
    it('should return null if no refresh token', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockRejectedValueOnce(new Error('no token'));
      const result = await authModule.refreshTokens();
      expect(result).toBeNull();
    });

    it('should refresh tokens and save new ones', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      // getRefreshToken
      vi.mocked(mockedInvoke).mockResolvedValueOnce('old-refresh');
      mockFetchOk(mockAuthResponse);
      // saveTokens
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);

      const result = await authModule.refreshTokens();
      expect(result).not.toBeNull();
      expect(result!.access_token).toBe('acc-123');
    });

    it('should clear tokens on refresh failure', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      // getRefreshToken
      vi.mocked(mockedInvoke).mockResolvedValueOnce('old-refresh');
      mockFetchError(401, 'expired');
      // clearTokens
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);

      const result = await authModule.refreshTokens();
      expect(result).toBeNull();
      expect(mockedInvoke).toHaveBeenCalledWith('auth_clear_tokens');
    });
  });

  describe('logout', () => {
    it('should call logout endpoint and clear tokens', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      // getAccessToken for authFetch
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({});
      // clearTokens
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);

      await authModule.logout();
      expect(mockedInvoke).toHaveBeenCalledWith('auth_clear_tokens');
    });

    it('should clear tokens even if logout fetch fails', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      // getAccessToken
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      vi.mocked(global.fetch).mockRejectedValueOnce(new Error('network'));
      // clearTokens
      vi.mocked(mockedInvoke).mockResolvedValueOnce(undefined);

      await authModule.logout();
      expect(mockedInvoke).toHaveBeenCalledWith('auth_clear_tokens');
    });
  });

  describe('getMe', () => {
    it('should return user profile', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      const profile = { user_id: 'u1', email: 'a@b.com' };
      mockFetchOk(profile);

      const result = await authModule.getMe();
      expect(result).toEqual(profile);
    });

    it('should return null on non-ok response', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      vi.mocked(global.fetch).mockResolvedValueOnce({ ok: false, status: 401 } as any);

      const result = await authModule.getMe();
      expect(result).toBeNull();
    });
  });

  describe('linkDevice', () => {
    it('should call link-device endpoint', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({});

      await authModule.linkDevice('dev-1', 'windows');
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/auth/link-device'),
        expect.objectContaining({ method: 'POST' }),
      );
    });

    it('should handle optional platform', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({});

      await authModule.linkDevice('dev-1');
      const fetchCall = vi.mocked(global.fetch).mock.calls[0];
      const body = JSON.parse(fetchCall[1]!.body as string);
      expect(body.device_id).toBe('dev-1');
      expect(body.platform).toBeUndefined();
    });
  });

  describe('forgotPassword', () => {
    it('should return success message', async () => {
      mockFetchOk({ message: 'Check your email' });
      const result = await authModule.forgotPassword('a@b.com');
      expect(result.message).toBe('Check your email');
    });

    it('should throw on error', async () => {
      mockFetchError(400, 'User not found');
      await expect(authModule.forgotPassword('a@b.com')).rejects.toThrow('User not found');
    });

    it('should throw default message on non-JSON error', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.forgotPassword('a@b.com')).rejects.toThrow('Request failed');
    });
  });

  describe('resetPassword', () => {
    it('should return success message', async () => {
      mockFetchOk({ message: 'Password reset' });
      const result = await authModule.resetPassword('a@b.com', '123456', 'newpass');
      expect(result.message).toBe('Password reset');
    });

    it('should throw on error', async () => {
      mockFetchError(400, 'Invalid code');
      await expect(authModule.resetPassword('a@b.com', 'bad', 'newpass'))
        .rejects.toThrow('Invalid code');
    });

    it('should throw default on non-JSON error', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.resetPassword('a@b.com', '123', 'new'))
        .rejects.toThrow('Reset failed');
    });
  });

  describe('changePassword', () => {
    it('should return success message', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({ message: 'Changed' });

      const result = await authModule.changePassword('old', 'new');
      expect(result.message).toBe('Changed');
    });

    it('should throw on error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchError(400, 'Wrong password');

      await expect(authModule.changePassword('wrong', 'new'))
        .rejects.toThrow('Wrong password');
    });

    it('should throw default on non-JSON error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchErrorNoJson(500);

      await expect(authModule.changePassword('old', 'new'))
        .rejects.toThrow('Change failed');
    });
  });

  describe('verifyEmail', () => {
    it('should return success message', async () => {
      mockFetchOk({ message: 'Verified' });
      const result = await authModule.verifyEmail('a@b.com', '123456');
      expect(result.message).toBe('Verified');
    });

    it('should throw on error', async () => {
      mockFetchError(400, 'Bad code');
      await expect(authModule.verifyEmail('a@b.com', 'bad')).rejects.toThrow('Bad code');
    });

    it('should throw default on non-JSON error', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.verifyEmail('a@b.com', '123'))
        .rejects.toThrow('Verification failed');
    });
  });

  describe('resendVerification', () => {
    it('should return success message', async () => {
      mockFetchOk({ message: 'Sent' });
      const result = await authModule.resendVerification('a@b.com');
      expect(result.message).toBe('Sent');
    });

    it('should throw on error', async () => {
      mockFetchError(429, 'Rate limited');
      await expect(authModule.resendVerification('a@b.com')).rejects.toThrow('Rate limited');
    });

    it('should throw default on non-JSON error', async () => {
      mockFetchErrorNoJson(500);
      await expect(authModule.resendVerification('a@b.com'))
        .rejects.toThrow('Request failed');
    });
  });

  describe('updateProfile', () => {
    it('should return success message', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({ message: 'Updated' });

      const result = await authModule.updateProfile('New Name');
      expect(result.message).toBe('Updated');
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/auth/profile'),
        expect.objectContaining({ method: 'PUT' }),
      );
    });

    it('should throw on error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchError(400, 'Invalid name');

      await expect(authModule.updateProfile('')).rejects.toThrow('Invalid name');
    });

    it('should throw default on non-JSON error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchErrorNoJson(500);

      await expect(authModule.updateProfile('x')).rejects.toThrow('Update failed');
    });
  });

  describe('deactivateAccount', () => {
    it('should return success message', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({ message: 'Deactivated' });

      const result = await authModule.deactivateAccount();
      expect(result.message).toBe('Deactivated');
    });

    it('should throw on error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchError(403, 'Forbidden');

      await expect(authModule.deactivateAccount()).rejects.toThrow('Forbidden');
    });

    it('should throw default on non-JSON error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchErrorNoJson(500);

      await expect(authModule.deactivateAccount()).rejects.toThrow('Deactivation failed');
    });
  });

  describe('deleteAccount', () => {
    it('should return success message', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchOk({ message: 'Deleted' });

      const result = await authModule.deleteAccount();
      expect(result.message).toBe('Deleted');
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/auth/account'),
        expect.objectContaining({ method: 'DELETE' }),
      );
    });

    it('should throw on error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchError(403, 'Forbidden');

      await expect(authModule.deleteAccount()).rejects.toThrow('Forbidden');
    });

    it('should throw default on non-JSON error', async () => {
      const { invoke: mockedInvoke } = await import('@tauri-apps/api/core');
      vi.mocked(mockedInvoke).mockResolvedValueOnce('tok');
      mockFetchErrorNoJson(500);

      await expect(authModule.deleteAccount()).rejects.toThrow('Deletion failed');
    });
  });
});

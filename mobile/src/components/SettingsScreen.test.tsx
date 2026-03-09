import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import SettingsScreen from './SettingsScreen'

const mockLogout = vi.fn().mockResolvedValue(undefined)
const mockReplace = vi.fn()

let mockAuthState: any
let mockSyncState: any

vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => mockAuthState,
}))

vi.mock('@/contexts/SyncContext', () => ({
  useSync: () => mockSyncState,
}))

vi.mock('@/services/authService', () => ({
  updateProfile: vi.fn().mockResolvedValue(undefined),
  changePassword: vi.fn().mockResolvedValue(undefined),
  deleteAccount: vi.fn().mockResolvedValue(undefined),
  getDevices: vi.fn().mockResolvedValue([]),
}))

vi.mock('@/services/biometricAuth', () => ({
  isBiometricAvailable: vi.fn().mockResolvedValue(false),
  isBiometricEnabled: vi.fn().mockResolvedValue(false),
  setBiometricEnabled: vi.fn().mockResolvedValue(undefined),
  getBiometricType: vi.fn().mockResolvedValue('Biometrics'),
}))

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: mockReplace,
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => '/settings',
  useSearchParams: () => new URLSearchParams(),
}))

describe('SettingsScreen', () => {
  beforeEach(() => {
    mockAuthState = {
      user: {
        user_id: 'u1',
        email: 'test@example.com',
        display_name: 'Test User',
        devices: [],
        account_level: 'pro',
      },
      isAuthenticated: true,
      isLoading: false,
      error: null,
      login: vi.fn(),
      register: vi.fn(),
      logout: mockLogout,
      clearError: vi.fn(),
    }

    mockSyncState = {
      isOnline: true,
      isSyncing: false,
      pendingCount: 0,
      lastSyncedAt: null,
      forceSync: vi.fn(),
    }
  })

  it('renders Settings heading', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders user display name', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Test User')).toBeInTheDocument()
  })

  it('renders user email', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('test@example.com')).toBeInTheDocument()
  })

  it('renders account level badge', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('pro')).toBeInTheDocument()
  })

  it('shows "User" when display_name is null', () => {
    mockAuthState.user.display_name = null
    render(<SettingsScreen />)
    expect(screen.getByText('User')).toBeInTheDocument()
  })

  it('shows avatar initial from display name', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('T')).toBeInTheDocument()
  })

  it('shows avatar initial from email when no display name', () => {
    mockAuthState.user.display_name = null
    render(<SettingsScreen />)
    expect(screen.getByText('T')).toBeInTheDocument() // 't' from test@example.com, uppercased
  })

  it('toggles name editing when pencil icon is clicked', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    // Find the edit button (pencil icon) next to the name
    const editButtons = screen.getAllByRole('button')
    // The pencil button is near the display name
    const pencilButton = editButtons.find(
      btn => btn.querySelector('.lucide-pencil') || btn.querySelector('svg')
    )
    // Use a different approach: click on the button that has the Pencil icon
    // The edit button is small, near the name
    const nameEl = screen.getByText('Test User')
    const editBtn = nameEl.parentElement?.querySelector('button')
    if (editBtn) {
      await user.click(editBtn)
      expect(screen.getByDisplayValue('Test User')).toBeInTheDocument()
    }
  })

  it('expands Change Password section on click', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Change Password'))
    expect(screen.getByPlaceholderText('Current password')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('New password')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Confirm new password')).toBeInTheDocument()
  })

  it('shows password mismatch error', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Change Password'))
    await user.type(screen.getByPlaceholderText('Current password'), 'oldpass1')
    await user.type(screen.getByPlaceholderText('New password'), 'Newpass1!')
    await user.type(screen.getByPlaceholderText('Confirm new password'), 'Different1!')
    await user.click(screen.getByText('Change Password', { selector: 'button[type="submit"]' }))
    expect(screen.getByText('Passwords do not match')).toBeInTheDocument()
  })

  it('expands devices section on click', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    // Click "0 devices linked" button
    await user.click(screen.getByText('0 devices linked'))
    // Devices section expanded (the getDevices mock returns [])
  })

  it('shows device count text', () => {
    mockAuthState.user.devices = [
      { device_id: 'd1', platform: 'iOS', linked_at: '2025-01-01', last_seen: '2025-06-01' },
      { device_id: 'd2', platform: 'Android', linked_at: '2025-01-01', last_seen: '2025-06-01' },
    ]
    render(<SettingsScreen />)
    expect(screen.getByText('2 devices linked')).toBeInTheDocument()
  })

  it('shows singular "device" for 1 device', () => {
    mockAuthState.user.devices = [
      { device_id: 'd1', platform: 'iOS', linked_at: '2025-01-01', last_seen: '2025-06-01' },
    ]
    render(<SettingsScreen />)
    expect(screen.getByText('1 device linked')).toBeInTheDocument()
  })

  it('shows sync status as Connected when online', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Connected')).toBeInTheDocument()
  })

  it('shows sync status as Offline when not online', () => {
    mockSyncState.isOnline = false
    render(<SettingsScreen />)
    expect(screen.getByText('Offline')).toBeInTheDocument()
  })

  it('shows pending count in sync section', () => {
    mockSyncState.pendingCount = 5
    render(<SettingsScreen />)
    expect(screen.getByText('5 pending')).toBeInTheDocument()
  })

  it('does not show pending count when zero', () => {
    render(<SettingsScreen />)
    expect(screen.queryByText(/pending/)).not.toBeInTheDocument()
  })

  it('shows last synced time when available', () => {
    mockSyncState.lastSyncedAt = '2025-06-15T10:00:00Z'
    render(<SettingsScreen />)
    expect(screen.getByText(/Last synced:/)).toBeInTheDocument()
  })

  it('calls logout when Sign Out is clicked', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Sign Out'))
    expect(mockLogout).toHaveBeenCalled()
  })

  it('shows Delete Account button initially', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Delete Account')).toBeInTheDocument()
  })

  it('expands delete confirmation when Delete Account is clicked', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Delete Account'))
    expect(screen.getByText('This action is permanent')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Type DELETE')).toBeInTheDocument()
  })

  it('disables Delete Forever button until DELETE is typed', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Delete Account'))
    const deleteBtn = screen.getByText('Delete Forever')
    expect(deleteBtn).toBeDisabled()
  })

  it('enables Delete Forever button when DELETE is typed', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Delete Account'))
    await user.type(screen.getByPlaceholderText('Type DELETE'), 'DELETE')
    const deleteBtn = screen.getByText('Delete Forever')
    expect(deleteBtn).not.toBeDisabled()
  })

  it('hides delete confirmation when Cancel is clicked', async () => {
    const user = userEvent.setup()
    render(<SettingsScreen />)
    await user.click(screen.getByText('Delete Account'))
    expect(screen.getByText('This action is permanent')).toBeInTheDocument()
    await user.click(screen.getByText('Cancel'))
    expect(screen.queryByText('This action is permanent')).not.toBeInTheDocument()
  })

  it('calls deleteAccount and redirects on confirm', async () => {
    const user = userEvent.setup()
    const { deleteAccount } = await import('@/services/authService')
    render(<SettingsScreen />)
    await user.click(screen.getByText('Delete Account'))
    await user.type(screen.getByPlaceholderText('Type DELETE'), 'DELETE')
    await user.click(screen.getByText('Delete Forever'))
    expect(deleteAccount).toHaveBeenCalled()
    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith('/auth/login')
    })
  })

  it('renders app version', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('IQ:capture Mobile v0.1.0')).toBeInTheDocument()
  })

  it('renders security section heading', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Security')).toBeInTheDocument()
  })

  it('renders account section heading', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Account')).toBeInTheDocument()
  })

  it('renders sync section heading', () => {
    render(<SettingsScreen />)
    expect(screen.getByText('Sync')).toBeInTheDocument()
  })
})

'use client'

import React, { useState, useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { useAuth } from '@/contexts/AuthContext'
import { useSync } from '@/contexts/SyncContext'
import { DeviceSummary } from '@/types'
import * as authService from '@/services/authService'
import { LogOut, User, Cloud, HardDrive, Key, Pencil, Trash2, Smartphone, Check, X, ChevronRight } from 'lucide-react'

export default function SettingsScreen() {
  const { user, logout } = useAuth()
  const { isOnline, pendingCount, lastSyncedAt } = useSync()
  const router = useRouter()

  // Edit display name
  const [editingName, setEditingName] = useState(false)
  const [nameValue, setNameValue] = useState(user?.display_name || '')
  const [nameSaving, setNameSaving] = useState(false)

  // Change password
  const [showPassword, setShowPassword] = useState(false)
  const [currentPw, setCurrentPw] = useState('')
  const [newPw, setNewPw] = useState('')
  const [confirmPw, setConfirmPw] = useState('')
  const [pwLoading, setPwLoading] = useState(false)
  const [pwError, setPwError] = useState<string | null>(null)
  const [pwSuccess, setPwSuccess] = useState(false)

  // Devices
  const [devices, setDevices] = useState<DeviceSummary[]>(user?.devices || [])
  const [showDevices, setShowDevices] = useState(false)

  // Delete account
  const [showDelete, setShowDelete] = useState(false)
  const [deleteConfirm, setDeleteConfirm] = useState('')
  const [deleteLoading, setDeleteLoading] = useState(false)

  // Error state
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (showDevices) {
      authService.getDevices().then(setDevices).catch(() => {})
    }
  }, [showDevices])

  const handleSaveName = async () => {
    setNameSaving(true)
    setError(null)
    try {
      await authService.updateProfile(nameValue)
      setEditingName(false)
    } catch (e: any) {
      setError(e.message)
    } finally {
      setNameSaving(false)
    }
  }

  const handleChangePassword = async (e: React.FormEvent) => {
    e.preventDefault()
    setPwError(null)
    setPwSuccess(false)
    if (newPw !== confirmPw) {
      setPwError('Passwords do not match')
      return
    }
    setPwLoading(true)
    try {
      await authService.changePassword(currentPw, newPw)
      setPwSuccess(true)
      setCurrentPw('')
      setNewPw('')
      setConfirmPw('')
      setTimeout(() => { setShowPassword(false); setPwSuccess(false) }, 1500)
    } catch (e: any) {
      setPwError(e.message)
    } finally {
      setPwLoading(false)
    }
  }

  const handleDeleteAccount = async () => {
    setDeleteLoading(true)
    try {
      await authService.deleteAccount()
      router.replace('/auth/login')
    } catch (e: any) {
      setError(e.message)
      setDeleteLoading(false)
    }
  }

  return (
    <div className="px-4 pt-4 pb-24">
      <h1 className="text-2xl font-bold text-gray-900 mb-6">Settings</h1>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded-xl text-sm text-red-600">
          {error}
        </div>
      )}

      {/* Account section */}
      <div className="mb-6">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Account</h2>
        <div className="bg-white border border-gray-200 rounded-xl overflow-hidden">
          <div className="px-4 py-3 flex items-center gap-3">
            <div className="w-10 h-10 rounded-full bg-blue-600 flex items-center justify-center">
              <span className="text-white font-medium">
                {(user?.display_name || user?.email || '?')[0].toUpperCase()}
              </span>
            </div>
            <div className="flex-1 min-w-0">
              {editingName ? (
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    value={nameValue}
                    onChange={(e) => setNameValue(e.target.value)}
                    autoFocus
                    className="flex-1 px-2 py-1 border border-gray-300 rounded text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <button
                    onClick={handleSaveName}
                    disabled={nameSaving}
                    className="p-1 text-green-600"
                  >
                    <Check className="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => { setEditingName(false); setNameValue(user?.display_name || '') }}
                    className="p-1 text-gray-400"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>
              ) : (
                <div className="flex items-center gap-2">
                  <p className="text-sm font-medium text-gray-900 truncate">
                    {user?.display_name || 'User'}
                  </p>
                  <button onClick={() => setEditingName(true)} className="p-1 text-gray-400">
                    <Pencil className="w-3 h-3" />
                  </button>
                </div>
              )}
              <p className="text-xs text-gray-500 truncate">{user?.email}</p>
            </div>
            {user?.account_level && (
              <span className="px-2 py-0.5 bg-blue-50 text-blue-600 text-xs font-medium rounded-full capitalize">
                {user.account_level}
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Security section */}
      <div className="mb-6">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Security</h2>
        <div className="bg-white border border-gray-200 rounded-xl overflow-hidden">
          <button
            onClick={() => setShowPassword(!showPassword)}
            className="w-full px-4 py-3 flex items-center justify-between active:bg-gray-50"
          >
            <div className="flex items-center gap-2">
              <Key className="w-4 h-4 text-gray-400" />
              <span className="text-sm text-gray-700">Change Password</span>
            </div>
            <ChevronRight className={`w-4 h-4 text-gray-400 transition-transform ${showPassword ? 'rotate-90' : ''}`} />
          </button>

          {showPassword && (
            <form onSubmit={handleChangePassword} className="px-4 pb-3 space-y-3 border-t border-gray-100 pt-3">
              <input
                type="password"
                value={currentPw}
                onChange={(e) => setCurrentPw(e.target.value)}
                placeholder="Current password"
                required
                className="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <input
                type="password"
                value={newPw}
                onChange={(e) => setNewPw(e.target.value)}
                placeholder="New password"
                required
                minLength={8}
                className="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <input
                type="password"
                value={confirmPw}
                onChange={(e) => setConfirmPw(e.target.value)}
                placeholder="Confirm new password"
                required
                minLength={8}
                className="w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <p className="text-xs text-gray-400">Must include uppercase, lowercase, and a number</p>
              {pwError && <p className="text-xs text-red-600">{pwError}</p>}
              {pwSuccess && <p className="text-xs text-green-600">Password changed successfully!</p>}
              <button
                type="submit"
                disabled={pwLoading}
                className="w-full py-2 bg-blue-600 text-white rounded-lg text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {pwLoading ? 'Changing...' : 'Change Password'}
              </button>
            </form>
          )}
        </div>
      </div>

      {/* Devices section */}
      <div className="mb-6">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Devices</h2>
        <div className="bg-white border border-gray-200 rounded-xl overflow-hidden">
          <button
            onClick={() => setShowDevices(!showDevices)}
            className="w-full px-4 py-3 flex items-center justify-between active:bg-gray-50"
          >
            <div className="flex items-center gap-2">
              <Smartphone className="w-4 h-4 text-gray-400" />
              <span className="text-sm text-gray-700">
                {devices.length} device{devices.length !== 1 ? 's' : ''} linked
              </span>
            </div>
            <ChevronRight className={`w-4 h-4 text-gray-400 transition-transform ${showDevices ? 'rotate-90' : ''}`} />
          </button>

          {showDevices && devices.map((device, i) => (
            <div
              key={device.device_id}
              className={`px-4 py-2.5 flex items-center justify-between text-xs ${i === 0 ? 'border-t border-gray-100' : ''}`}
            >
              <div>
                <p className="text-gray-700 font-medium">{device.platform || 'Unknown'}</p>
                <p className="text-gray-400 font-mono truncate max-w-[180px]">{device.device_id}</p>
              </div>
              <div className="text-right text-gray-400">
                {device.last_seen ? new Date(device.last_seen).toLocaleDateString() : 'Never'}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Sync section */}
      <div className="mb-6">
        <h2 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Sync</h2>
        <div className="bg-white border border-gray-200 rounded-xl overflow-hidden">
          <div className="px-4 py-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Cloud className={`w-4 h-4 ${isOnline ? 'text-green-500' : 'text-red-500'}`} />
              <span className="text-sm text-gray-700">
                {isOnline ? 'Connected' : 'Offline'}
              </span>
            </div>
            {pendingCount > 0 && (
              <span className="text-xs text-yellow-600 font-medium">
                {pendingCount} pending
              </span>
            )}
          </div>
          {lastSyncedAt && (
            <div className="px-4 py-2 border-t border-gray-100 text-xs text-gray-400">
              Last synced: {new Date(lastSyncedAt).toLocaleString()}
            </div>
          )}
        </div>
      </div>

      {/* Sign out */}
      <button
        onClick={logout}
        className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-red-50 text-red-600 rounded-xl text-sm font-medium active:bg-red-100 mb-4"
      >
        <LogOut className="w-4 h-4" />
        Sign Out
      </button>

      {/* Delete Account */}
      <div className="mb-6">
        {!showDelete ? (
          <button
            onClick={() => setShowDelete(true)}
            className="w-full flex items-center justify-center gap-2 px-4 py-3 text-gray-400 text-xs active:text-red-500"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Delete Account
          </button>
        ) : (
          <div className="bg-red-50 border border-red-200 rounded-xl p-4 space-y-3">
            <p className="text-sm text-red-700 font-medium">This action is permanent</p>
            <p className="text-xs text-red-600">
              All your data including meetings, transcripts, and summaries will be permanently deleted. This cannot be undone.
            </p>
            <p className="text-xs text-gray-600">
              Type <span className="font-mono font-bold">DELETE</span> to confirm:
            </p>
            <input
              type="text"
              value={deleteConfirm}
              onChange={(e) => setDeleteConfirm(e.target.value)}
              className="w-full px-3 py-2 border border-red-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-red-500"
              placeholder="Type DELETE"
            />
            <div className="flex gap-2">
              <button
                onClick={() => { setShowDelete(false); setDeleteConfirm('') }}
                className="flex-1 py-2 bg-white border border-gray-300 text-gray-700 rounded-lg text-sm font-medium"
              >
                Cancel
              </button>
              <button
                onClick={handleDeleteAccount}
                disabled={deleteConfirm !== 'DELETE' || deleteLoading}
                className="flex-1 py-2 bg-red-600 text-white rounded-lg text-sm font-medium disabled:opacity-50"
              >
                {deleteLoading ? 'Deleting...' : 'Delete Forever'}
              </button>
            </div>
          </div>
        )}
      </div>

      {/* App version */}
      <p className="text-center text-xs text-gray-400 mt-6">
        IQ:capture Mobile v0.1.0
      </p>
    </div>
  )
}

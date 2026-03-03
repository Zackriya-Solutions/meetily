'use client'

import React from 'react'
import { useAuth } from '@/contexts/AuthContext'
import { useSync } from '@/contexts/SyncContext'
import { LogOut, User, Cloud, HardDrive } from 'lucide-react'

export default function SettingsScreen() {
  const { user, logout } = useAuth()
  const { isOnline, pendingCount, lastSyncedAt } = useSync()

  return (
    <div className="px-4 pt-4">
      <h1 className="text-2xl font-bold text-gray-900 mb-6">Settings</h1>

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
              <p className="text-sm font-medium text-gray-900 truncate">
                {user?.display_name || 'User'}
              </p>
              <p className="text-xs text-gray-500 truncate">{user?.email}</p>
            </div>
          </div>
          <div className="px-4 py-3 border-t border-gray-100 flex items-center gap-2 text-xs text-gray-500">
            <HardDrive className="w-3.5 h-3.5" />
            <span>{user?.devices?.length || 0} device{(user?.devices?.length || 0) !== 1 ? 's' : ''} linked</span>
          </div>
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
        className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-red-50 text-red-600 rounded-xl text-sm font-medium active:bg-red-100"
      >
        <LogOut className="w-4 h-4" />
        Sign Out
      </button>

      {/* App version */}
      <p className="text-center text-xs text-gray-400 mt-6">
        IQ:capture Mobile v0.1.0
      </p>
    </div>
  )
}

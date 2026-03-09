'use client'

import React, { useState, useRef, useEffect } from 'react'
import { useAuth } from '@/contexts/AuthContext'
import { ChangePasswordForm } from './ChangePasswordForm'
import { updateProfile, deactivateAccount, deleteAccount } from '@/services/authService'

export function UserProfileMenu() {
  const { user, logout, isAuthenticated } = useAuth()
  const [open, setOpen] = useState(false)
  const [showChangePassword, setShowChangePassword] = useState(false)
  const [editingName, setEditingName] = useState(false)
  const [newName, setNewName] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  // Close on outside click
  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    if (open) document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  if (!isAuthenticated || !user) return null

  const initial = (user.display_name || user.email)[0].toUpperCase()

  const handleSaveName = async () => {
    try {
      await updateProfile(newName)
      setEditingName(false)
      // Reload will pick up the new name via /me
      window.location.reload()
    } catch {
      // silent
    }
  }

  const handleDeactivate = async () => {
    try {
      await deactivateAccount()
      await logout()
    } catch {
      // silent
    }
  }

  const handleDelete = async () => {
    try {
      await deleteAccount()
      await logout()
    } catch {
      // silent
    }
  }

  return (
    <>
      <div ref={menuRef} className="relative">
        <button
          onClick={() => setOpen(!open)}
          className="flex items-center gap-2 px-2 py-1 rounded-md hover:bg-gray-100 text-sm"
        >
          <span className="w-6 h-6 rounded-full bg-blue-600 text-white flex items-center justify-center text-xs font-medium">
            {initial}
          </span>
          <span className="text-gray-700 truncate max-w-[120px]">
            {user.display_name || user.email}
          </span>
        </button>

        {open && (
          <div className="absolute bottom-full left-0 mb-1 w-56 bg-white rounded-md shadow-lg border border-gray-200 py-1 z-50">
            <div className="px-3 py-2 border-b border-gray-100">
              {editingName ? (
                <div className="flex gap-1">
                  <input
                    type="text"
                    value={newName}
                    onChange={e => setNewName(e.target.value)}
                    className="flex-1 px-1 py-0.5 border border-gray-300 rounded text-sm"
                    placeholder="Display name"
                    autoFocus
                  />
                  <button onClick={handleSaveName} className="text-xs text-blue-600">Save</button>
                  <button onClick={() => setEditingName(false)} className="text-xs text-gray-400">Cancel</button>
                </div>
              ) : (
                <>
                  <p className="text-sm font-medium text-gray-900 truncate">
                    {user.display_name || 'User'}
                    <button
                      onClick={() => { setNewName(user.display_name || ''); setEditingName(true) }}
                      className="ml-1 text-xs text-blue-600 hover:underline"
                    >
                      Edit
                    </button>
                  </p>
                  <p className="text-xs text-gray-500 truncate">{user.email}</p>
                </>
              )}
            </div>
            <div className="px-3 py-1.5 text-xs text-gray-400">
              {user.devices.length} device{user.devices.length !== 1 ? 's' : ''} linked
              {user.account_level && (
                <span className="ml-1 capitalize">({user.account_level})</span>
              )}
            </div>
            <button
              onClick={() => {
                setOpen(false)
                setShowChangePassword(true)
              }}
              className="w-full text-left px-3 py-2 text-sm text-gray-700 hover:bg-gray-50"
            >
              Change Password
            </button>
            <button
              onClick={async () => {
                setOpen(false)
                await logout()
              }}
              className="w-full text-left px-3 py-2 text-sm text-red-600 hover:bg-red-50"
            >
              Sign Out
            </button>
            <div className="border-t border-gray-100 mt-1 pt-1">
              {!confirmDelete ? (
                <button
                  onClick={() => setConfirmDelete(true)}
                  className="w-full text-left px-3 py-2 text-xs text-gray-400 hover:text-red-600 hover:bg-red-50"
                >
                  Delete Account
                </button>
              ) : (
                <div className="px-3 py-2 space-y-1">
                  <p className="text-xs text-red-600 font-medium">This cannot be undone.</p>
                  <div className="flex gap-2">
                    <button
                      onClick={handleDelete}
                      className="text-xs text-white bg-red-600 rounded px-2 py-1 hover:bg-red-700"
                    >
                      Confirm Delete
                    </button>
                    <button
                      onClick={() => setConfirmDelete(false)}
                      className="text-xs text-gray-500 hover:text-gray-700"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {showChangePassword && (
        <ChangePasswordForm onClose={() => setShowChangePassword(false)} />
      )}
    </>
  )
}

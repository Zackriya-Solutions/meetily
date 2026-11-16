'use client'

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

export function SummaryTimeoutSettings() {
  const [seconds, setSeconds] = useState('')
  const [loaded, setLoaded] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    let mounted = true
    invoke<number | null>('api_get_summary_timeout').then(value => {
      if (mounted) {
        setSeconds(value === null ? '' : String(value))
        setLoaded(true)
      }
    }).catch(() => {
      if (mounted) setError('Could not load the summary request timeout. Reopen Settings to try again.')
    })
    return () => { mounted = false }
  }, [])

  const save = async () => {
    const value = seconds.trim() === '' ? null : Number(seconds)
    if (value !== null && (!Number.isInteger(value) || value < 1 || value > 4294967295)) {
      setError('Enter the summary request timeout as a whole number from 1 to 4294967295 seconds, or leave it blank for the default.')
      return
    }
    setSaving(true)
    setError('')
    setSaved(false)
    try {
      await invoke('api_save_summary_timeout', { seconds: value })
      setSaved(true)
    } catch {
      setError('Could not save the summary request timeout. Select Save timeout to try again.')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
      <h3 className="text-lg font-semibold text-gray-900 mb-2">Summary Timeout</h3>
      <p id="summary-timeout-hint" className="text-sm text-gray-600 mb-4">
        Set how long to wait for each summary request. Longer transcripts may need several requests.
        Leave blank to use 300 seconds, or 900 seconds for built-in models. Changes apply to new generations.
      </p>
      <label htmlFor="summary-timeout" className="block text-sm font-medium mb-2">
        Summary request timeout (seconds)
      </label>
      <div className="flex items-center gap-3">
        <Input
          id="summary-timeout"
          type="number"
          min={1}
          max={4294967295}
          step={1}
          placeholder="Provider default"
          value={seconds}
          disabled={!loaded || saving}
          aria-describedby={`summary-timeout-hint${error ? ' summary-timeout-error' : ''}`}
          aria-invalid={!!error}
          className="max-w-xs"
          onChange={event => {
            setSeconds(event.target.value)
            setError('')
            setSaved(false)
          }}
        />
        <Button disabled={!loaded || saving} onClick={save}>
          {saving ? 'Saving…' : 'Save timeout'}
        </Button>
      </div>
      {error && <p id="summary-timeout-error" role="alert" className="text-sm text-red-600 mt-2">{error}</p>}
      <p role="status" className="text-sm text-gray-600 mt-2">{saved ? 'Summary request timeout saved.' : ''}</p>
    </div>
  )
}

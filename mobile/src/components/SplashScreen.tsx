'use client'

import React from 'react'

export default function SplashScreen() {
  return (
    <div
      className="fixed inset-0 z-[100] flex flex-col items-center justify-center"
      style={{
        background: 'linear-gradient(135deg, #2276aa, #1caac9)',
        paddingTop: 'env(safe-area-inset-top, 0px)',
      }}
    >
      <div className="flex flex-col items-center gap-4">
        <div className="w-20 h-20 bg-white/20 rounded-2xl flex items-center justify-center">
          <span className="text-white text-3xl font-bold">iQ</span>
        </div>
        <h1 className="text-white text-3xl font-bold tracking-iq">IQ:capture</h1>
        <p className="text-white/60 text-sm">Meeting Intelligence</p>
        <div className="mt-8 w-6 h-6 border-2 border-white/30 border-t-white rounded-full animate-spin" />
      </div>
    </div>
  )
}

'use client'

import React from 'react'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { List, Mic, Settings } from 'lucide-react'

const tabs = [
  { href: '/', label: 'Meetings', icon: List },
  { href: '/record', label: 'Record', icon: Mic },
  { href: '/settings', label: 'Settings', icon: Settings },
]

export default function TabBar() {
  const pathname = usePathname()

  return (
    <nav className="fixed bottom-0 left-0 right-0 bg-iq-light border-t border-iq-light-shade pb-[var(--safe-area-bottom)] z-50">
      <div className="flex items-center justify-around h-14">
        {tabs.map(({ href, label, icon: Icon }) => {
          const active = href === '/'
            ? pathname === '/'
            : pathname.startsWith(href)

          return (
            <Link
              key={href}
              href={href}
              className={`flex flex-col items-center gap-0.5 px-4 py-1 transition-colors ${
                active ? 'text-iq-blue' : 'text-iq-medium'
              }`}
            >
              <Icon className="w-5 h-5" />
              <span className="text-[10px] font-semibold">{label}</span>
            </Link>
          )
        })}
      </div>
    </nav>
  )
}

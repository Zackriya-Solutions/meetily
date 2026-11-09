'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { MessageSquareText } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip';

export default function FeedbackLink({ isCollapsed }: { isCollapsed: boolean }) {
  const pathname = usePathname();
  const isActive = pathname === '/feedback';
  const link = (
    <Link
      href="/feedback"
      replace={pathname === '/settings'}
      aria-label="Send Feedback"
      aria-current={isActive ? 'page' : undefined}
      className={`flex items-center justify-center rounded-lg transition-colors active:bg-gray-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gray-500 focus-visible:ring-offset-2 ${
        isCollapsed
          ? 'p-2'
          : 'w-full px-3 py-1.5 mt-1 mb-2 text-sm font-medium shadow-sm'
      } ${
        isCollapsed
          ? `text-gray-600 ${isActive ? 'bg-gray-100' : 'hover:bg-gray-100'}`
          : 'bg-gray-200 text-gray-700 hover:bg-gray-300'
      }`}
    >
      <MessageSquareText aria-hidden="true" className={isCollapsed ? 'w-5 h-5' : 'w-4 h-4'} />
      {!isCollapsed && <span className="ml-2">Send Feedback</span>}
    </Link>
  );

  if (!isCollapsed) return link;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{link}</TooltipTrigger>
      <TooltipContent side="right">Send Feedback</TooltipContent>
    </Tooltip>
  );
}
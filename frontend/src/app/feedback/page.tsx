'use client';

import { ArrowLeft, ArrowUpRight } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

const cards = [
  {
    title: 'Report a bug or suggest a feature',
    description: 'Report a bug or suggest a feature on GitHub. Your feedback helps shape Meetily.',
    links: [{ label: 'Open GitHub issues', url: 'https://github.com/Zackriya-Solutions/meetily/issues/new/choose' }],
  },
  {
    title: 'Join us on Discord',
    description: 'Ask questions, get help, and share ideas with people using and building Meetily.',
    links: [{ label: 'Join the server', url: 'https://discord.gg/crRymMQBFH' }],
  },
  {
    title: 'Find us on socials',
    description: 'Follow Meetily for product updates and conversations with the community.',
    links: [
      { label: 'X', url: 'https://x.com/meetilyai/', icon: 'x' },
      { label: 'Reddit', url: 'https://www.reddit.com/r/meetily/', icon: 'reddit' },
      { label: 'LinkedIn', url: 'https://www.linkedin.com/company/106363062/', icon: 'linkedin' },
      { label: 'YouTube', url: 'https://www.youtube.com/@meetily', icon: 'youtube' },
    ],
  },
  {
    title: 'Evaluating Meetily for your team?',
    description: 'Explore deployment and support options for using Meetily across your organization.',
    links: [{ label: 'Explore team options', url: 'https://meetily.ai/enterprise?utm_source=open-source' }],
  },
  {
    title: 'Need more from Meetily?',
    description: 'Get speaker identification, automatic meeting detection, and API, CLI, and MCP access with Meetily PRO.',
    links: [{ label: 'Explore PRO', url: 'https://meetily.ai/pro?utm_source=open-source' }],
  },
];

async function openExternalLink(url: string) {
  try {
    await invoke('open_external_url', { url });
  } catch (error) {
    console.error('Failed to open feedback link:', error);
    toast.error('Could not open this link. Please check your default browser and try again.');
  }
}

export default function FeedbackPage() {
  const router = useRouter();

  return (
    <div className="h-screen overflow-y-auto bg-gray-50">
      <div className="mx-auto w-full max-w-6xl px-8 py-8">
        <button
          type="button"
          onClick={() => router.back()}
          className="flex items-center gap-2 rounded text-gray-600 transition-colors hover:text-gray-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2"
        >
          <ArrowLeft aria-hidden="true" className="h-4 w-4" />
          Back
        </button>
        <h1 className="mt-6 mb-8 text-2xl font-medium text-gray-900">Send Feedback</h1>

        <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,300px),1fr))] gap-5">
          {cards.map(card => (
            <article key={card.title} className="flex min-h-40 flex-col rounded-lg border border-gray-200 bg-white p-5 pb-3">
              <h2 className="text-lg font-semibold leading-6 text-gray-900">{card.title}</h2>
              <p className="mt-2 text-sm leading-5 text-gray-500">{card.description}</p>
              <div className="mt-auto flex flex-wrap items-center gap-x-6">
                {card.links.map(link => (
                  <button
                    key={link.url}
                    type="button"
                    onClick={() => void openExternalLink(link.url)}
                    aria-label={`${link.label}`}
                    title={`${link.label}`}
                    className={`inline-flex min-h-11 items-center justify-center gap-1 rounded text-sm font-semibold text-gray-700 transition-colors hover:text-gray-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2 ${'icon' in link ? 'min-w-11' : ''}`}
                  >
                    {'icon' in link ? (
                      <svg aria-hidden="true" focusable="false" className="h-5 w-5">
                        <use href={`/social-icons.svg#${link.icon}`} />
                      </svg>
                    ) : (
                      <>
                        {link.label}
                        <ArrowUpRight aria-hidden="true" className="h-3 w-3" />
                      </>
                    )}
                  </button>
                ))}
              </div>
            </article>
          ))}
        </div>
      </div>
    </div>
  );
}

import './globals.css';
import type { Metadata } from 'next';
import { Source_Sans_3, Syne } from 'next/font/google';
import ClientRoot from './ClientRoot';

const sourceSans3 = Source_Sans_3({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-source-sans-3',
});

const syne = Syne({
  subsets: ['latin'],
  weight: ['400', '600', '700', '800'],
  variable: '--font-syne',
});

export const metadata: Metadata = {
  title: 'Clearminutes',
  description: 'Record. Transcribe. Summarize. All on your device.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${sourceSans3.variable} ${syne.variable} font-sans antialiased`}>
        <ClientRoot>{children}</ClientRoot>
      </body>
    </html>
  );
}

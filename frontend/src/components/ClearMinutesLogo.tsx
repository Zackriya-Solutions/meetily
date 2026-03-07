'use client';

import { useTheme } from '@/contexts/ThemeContext';

/**
 * The Clearminutes brand icon SVG — recolours automatically based on the active theme.
 * Arc gradient and glow shift: teal/blue (dark), purple/pink (dracula), blue/indigo (midnight), teal/blue-muted (light).
 */
export function ClearMinutesIcon({ size = 32, className = '' }: { size?: number; className?: string }) {
  const { theme } = useTheme();

  const configs = {
    light: {
      rect: ['#ffffff', '#f0f0f8'],
      arcFrom: '#00a884', arcTo: '#0077cc',
      dotRing: '#0077cc', dotRingOpacity: 0.18,
      hexStroke: '#00a884', hexOpacity: 0.25,
      tickStroke: '#00a884',
      centerFill: ['#00a884', '#0077cc'],
      topDot: '#00a884',
      rectStroke: '#e2e2ec',
    },
    dark: {
      rect: ['#25272f', '#2c2e38'],
      arcFrom: '#00d4aa', arcTo: '#0099ff',
      dotRing: '#0099ff', dotRingOpacity: 0.35,
      hexStroke: '#00d4aa', hexOpacity: 0.35,
      tickStroke: '#00d4aa',
      centerFill: ['#00d4aa', '#0099ff'],
      topDot: '#00d4aa',
      rectStroke: null,
    },
    dracula: {
      rect: ['#2d2f3d', '#343746'],
      arcFrom: '#bd93f9', arcTo: '#ff79c6',
      dotRing: '#ff79c6', dotRingOpacity: 0.3,
      hexStroke: '#bd93f9', hexOpacity: 0.35,
      tickStroke: '#bd93f9',
      centerFill: ['#bd93f9', '#ff79c6'],
      topDot: '#bd93f9',
      rectStroke: null,
    },
    midnight: {
      rect: ['#141720', '#1a1e2a'],
      arcFrom: '#60a5fa', arcTo: '#818cf8',
      dotRing: '#818cf8', dotRingOpacity: 0.3,
      hexStroke: '#60a5fa', hexOpacity: 0.35,
      tickStroke: '#60a5fa',
      centerFill: ['#60a5fa', '#818cf8'],
      topDot: '#60a5fa',
      rectStroke: null,
    },
  };

  const c = configs[theme] ?? configs.dark;
  const id = `cm-${theme}`; // unique gradient IDs per theme

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 120 120"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <defs>
        <linearGradient id={`${id}-bg`} x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor={c.rect[0]} />
          <stop offset="100%" stopColor={c.rect[1]} />
        </linearGradient>
        <linearGradient id={`${id}-arc`} x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={c.arcFrom} />
          <stop offset="100%" stopColor={c.arcTo} />
        </linearGradient>
        <linearGradient id={`${id}-center`} x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor={c.centerFill[0]} />
          <stop offset="100%" stopColor={c.centerFill[1]} />
        </linearGradient>
        <filter id={`${id}-glow`}>
          <feGaussianBlur stdDeviation="2.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {/* Background rect */}
      <rect
        width="120" height="120" rx="27"
        fill={`url(#${id}-bg)`}
        stroke={c.rectStroke ?? 'none'}
        strokeWidth={c.rectStroke ? 2 : 0}
      />

      {/* Dashed orbit ring */}
      <circle
        cx="60" cy="60" r="36"
        fill="none"
        stroke={c.dotRing}
        strokeWidth="1"
        strokeDasharray="4 3"
        opacity={c.dotRingOpacity}
      />

      {/* Main arc — most of circle, glowing */}
      <path
        d="M 60 28 A 32 32 0 1 1 28 60"
        fill="none"
        stroke={`url(#${id}-arc)`}
        strokeWidth="3.5"
        strokeLinecap="round"
        filter={`url(#${id}-glow)`}
      />

      {/* Hexagon / clock-face polygon */}
      <polygon
        points="60,38 74,46 74,62 60,70 46,62 46,46"
        fill="none"
        stroke={c.hexStroke}
        strokeWidth="1.5"
        strokeOpacity={c.hexOpacity}
      />

      {/* 12 o'clock tick mark */}
      <line
        x1="60" y1="25" x2="60" y2="31"
        stroke={c.tickStroke}
        strokeWidth="2"
        strokeLinecap="round"
      />

      {/* Centre dot */}
      <circle
        cx="60" cy="60" r="4"
        fill={`url(#${id}-center)`}
        filter={`url(#${id}-glow)`}
      />

      {/* Top arc anchor dot */}
      <circle cx="60" cy="28" r="2" fill={c.topDot} />
    </svg>
  );
}

/**
 * Full wordmark: icon + "clearminutes" text, sized for the sidebar.
 */
export function ClearMinutesWordmark({ iconSize = 28, className = '' }: { iconSize?: number; className?: string }) {
  return (
    <span className={`flex items-center gap-2 ${className}`}>
      <ClearMinutesIcon size={iconSize} />
      <span
        className="font-bold tracking-tight text-gray-900 dark:text-foreground"
        style={{ fontFamily: 'var(--font-syne, system-ui)', fontSize: iconSize * 0.65, letterSpacing: '-0.02em' }}
      >
        clearminutes
      </span>
    </span>
  );
}


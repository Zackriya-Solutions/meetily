import type { Config } from 'tailwindcss'

const config: Config = {
  content: [
    './src/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['var(--font-poppins)', 'system-ui', 'sans-serif'],
      },
      colors: {
        iq: {
          blue: '#2b92d0',
          'blue-dark': '#2276aa',
          'blue-light': '#55a8ff',
          orange: '#f7931d',
          'orange-dark': '#f76b1d',
          'orange-light': '#f9a94a',
          purple: '#971df7',
          green: '#2d850f',
          red: '#d40000',
          dark: '#3c3c3b',
          'dark-shade': '#343433',
          medium: '#6f6f6e',
          light: '#f8f8f8',
          'light-shade': '#d7d8da',
        },
      },
      letterSpacing: {
        iq: '-0.01em',
      },
      borderRadius: {
        'iq-sm': '8px',
        'iq-md': '10px',
        'iq-lg': '12px',
      },
    },
  },
  plugins: [
    require('@tailwindcss/typography'),
    require('tailwindcss-animate'),
  ],
}

export default config

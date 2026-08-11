/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        base: { 900: '#0b0d10', 800: '#12151a', 700: '#1a1f26', 600: '#232a33', 500: '#2f3843' },
        accent: { DEFAULT: '#4f9cf9', dim: '#2f6fbd' },
        danger: '#e5484d',
        warn: '#f5a524',
        ok: '#30a46c',
      },
      fontFamily: {
        mono: ['ui-monospace', 'SFMono-Regular', 'Cascadia Code', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
};

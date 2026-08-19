/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        surface: '#0d1117',
        panel:   '#161b22',
        border:  '#30363d',
        muted:   '#8b949e',
        accent:  '#58a6ff',
      },
    },
  },
  plugins: [],
}

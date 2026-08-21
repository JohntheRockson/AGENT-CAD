export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        // Core surfaces
        surface: '#0e0e16',
        panel:   '#141420',
        raised:  '#1a1a28',
        sunken:  '#0a0a12',
        // Borders / dividers
        border:  '#252538',
        divide:  '#1c1c2e',
        // Text
        muted:   '#7878a0',
        dim:     '#3a3a58',
        // Primary accent
        accent:  '#5b8cff',
        'accent-lite': '#8ab0ff',
        // Semantic
        green:   '#3ec96a',
        amber:   '#f0a830',
        red:     '#e84040',
        cyan:    '#30c8d8',
        // Viewport
        vp:      '#0a0a14',
      },
      boxShadow: {
        'cad': '0 2px 8px rgba(0,0,0,0.5)',
        'cad-lg': '0 8px 32px rgba(0,0,0,0.6)',
        'glow': '0 0 12px rgba(91,140,255,0.25)',
      },
    },
  },
  plugins: [],
}

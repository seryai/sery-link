/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      // Brand purple — overrides Tailwind's default palette so every
      // existing `bg-purple-600` / `text-purple-700` / etc. resolves
      // to brand-aligned values without code changes. 600 is the
      // canonical brand purple #5b3ea3 (matches the website + the
      // app icon + the favicons). Other stops keep the same hue with
      // lightness varied for the standard Tailwind ramp.
      colors: {
        purple: {
          50:  "#f5f3fb",
          100: "#ebe5f7",
          200: "#d2c5ed",
          300: "#b8a3e1",
          400: "#9077c8",
          500: "#6e4cb0",
          600: "#5b3ea3",
          700: "#4c2d8c",
          800: "#3b2670",
          900: "#2d1d56",
          950: "#1a1135",
        },
      },
      keyframes: {
        pulse_ring: {
          '0%': { transform: 'scale(0.8)', opacity: '1' },
          '100%': { transform: 'scale(2)', opacity: '0' },
        },
        slide_up: {
          '0%': { transform: 'translateY(20px)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
      animation: {
        pulse_ring: 'pulse_ring 1.5s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        slide_up: 'slide_up 200ms ease-out',
      },
    },
  },
  plugins: [],
}

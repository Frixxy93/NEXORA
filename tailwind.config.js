/** @type {import('tailwindcss').Config} */
export default {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // NEXORA dark surface system
        ink: {
          900: "#0b0d10", // app background
          850: "#0f1216",
          800: "#14181d", // panels
          750: "#1a1f26",
          700: "#232a33", // cards / borders
          600: "#2f3843",
        },
        line: "#2a323c",
        accent: {
          DEFAULT: "#e0803a", // NEXORA amber (material shelf warmth)
          soft: "#f0a35e",
          dim: "#8a5326",
        },
        good: "#4ea375",
        warn: "#d9a441",
        bad: "#cf5b5b",
        muted: "#7b8794",
      },
      fontFamily: {
        sans: [
          "Inter",
          "-apple-system",
          "Segoe UI",
          "Roboto",
          "Helvetica",
          "Arial",
          "sans-serif",
        ],
        mono: ["JetBrains Mono", "SFMono-Regular", "Consolas", "monospace"],
      },
    },
  },
  plugins: [],
};

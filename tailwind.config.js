/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        border: "hsl(220 14% 88%)",
        panel: "hsl(0 0% 100%)",
        muted: "hsl(220 14% 96%)",
        ink: "hsl(222 34% 11%)",
        subdued: "hsl(220 9% 46%)",
        accent: "hsl(158 64% 34%)",
        amber: "hsl(38 92% 50%)",
      },
      boxShadow: {
        soft: "0 1px 2px rgba(15, 23, 42, 0.08)",
      },
    },
  },
  plugins: [],
};

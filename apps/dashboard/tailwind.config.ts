import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Sentinel Dark theme
        "sentinel-dark": {
          bg: "#0a0f1a",
          surface: "#101827",
          primary: "#00f3ff",
          success: "#52ffa8",
          muted: "#7a8aa0",
          warning: "#ff8f6b",
        },
        // Backwards-compatible aliases
        ink: "#0a0f1a",
        signal: "#52ffa8",
        pulse: "#00f3ff",
        warning: "#ff8f6b",
        cyan: {
          electric: "#00f3ff",
        },
      },
      fontFamily: {
        display: ["ui-serif", "Georgia", "serif"],
        body: ["ui-sans-serif", "system-ui", "sans-serif"],
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },
      boxShadow: {
        panel: "0 20px 50px rgba(4, 9, 18, 0.35)",
        glow: "0 0 20px rgba(0, 229, 255, 0.15)",
        "glow-signal": "0 0 24px rgba(82, 255, 168, 0.12)",
      },
      keyframes: {
        flash: {
          "0%": { backgroundColor: "rgba(82, 255, 168, 0.08)" },
          "100%": { backgroundColor: "transparent" },
        },
        shake: {
          "0%, 100%": { transform: "translateX(0)" },
          "15%": { transform: "translateX(-3px)" },
          "30%": { transform: "translateX(3px)" },
          "45%": { transform: "translateX(-2px)" },
          "60%": { transform: "translateX(2px)" },
          "75%": { transform: "translateX(-1px)" },
          "90%": { transform: "translateX(1px)" },
        },
        "fade-in": {
          "0%": { opacity: "0", transform: "scale(0.98) translateY(4px)" },
          "100%": { opacity: "1", transform: "scale(1) translateY(0)" },
        },
        "slide-up": {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
      },
      animation: {
        flash: "flash 0.7s ease-out",
        shake: "shake 0.4s ease-in-out",
        "fade-in": "fade-in 0.25s ease-out",
        "slide-up": "slide-up 0.2s ease-out",
      },
    },
  },
  plugins: [],
};

export default config;

import type { ButtonHTMLAttributes, ReactNode } from "react";
import { playSfx } from "../../lib/sfx";

type Variant = "primary" | "secondary" | "danger" | "ghost";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  children: ReactNode;
  sfx?: string | false;
}

const variants: Record<Variant, string> = {
  primary: "border-emerald-500/50 bg-emerald-500 text-zinc-950 hover:bg-emerald-400 hover:shadow-[0_0_16px_rgba(16,185,129,0.35)] hover:-translate-y-px",
  secondary: "border-zinc-700/80 bg-zinc-900 text-zinc-100 hover:bg-zinc-800 hover:border-zinc-600 hover:-translate-y-px",
  danger: "border-red-500/40 bg-red-950/80 text-red-100 hover:bg-red-900 hover:border-red-500/60 hover:-translate-y-px",
  ghost: "border-transparent bg-transparent text-zinc-300 hover:bg-zinc-900 hover:text-zinc-100",
};

export function Button({ className = "", variant = "secondary", children, sfx = "button-click-else", onClick, ...props }: ButtonProps) {
  return (
    <button
      className={`inline-flex h-9 items-center justify-center gap-2 rounded-lg border px-3 text-sm font-medium transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-emerald-500/50 disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:translate-y-0 ${variants[variant]} ${className}`}
      onClick={(e) => {
        if (sfx) playSfx(sfx);
        onClick?.(e);
      }}
      {...props}
    >
      {children}
    </button>
  );
}


import type { HTMLAttributes, ReactNode } from "react";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

export function Card({ className = "", children, ...props }: CardProps) {
  return (
    <div className={`rounded-xl border border-zinc-800/70 bg-zinc-950 transition-all duration-200 ${className}`} {...props}>
      {children}
    </div>
  );
}


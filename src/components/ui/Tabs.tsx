import type { ReactNode } from "react";
import { playSfx } from "../../lib/sfx";

interface TabsProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  tabs: Array<{ value: T; label: string; icon?: ReactNode; disabled?: boolean; tooltip?: string }>;
}

export function Tabs<T extends string>({ value, onValueChange, tabs }: TabsProps<T>) {
  return (
    <div className="flex gap-1 rounded-xl border border-zinc-800/70 bg-zinc-950 p-1">
      {tabs.map((tab) => {
        const isDisabled = tab.disabled;
        const isActive = value === tab.value;
        const btnClass = isDisabled
          ? "inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-lg text-sm font-medium transition-all cursor-not-allowed text-zinc-600 opacity-50"
          : isActive
            ? "inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-lg text-sm font-medium transition-all bg-zinc-900 border border-zinc-700 text-emerald-400 shadow-[0_2px_0_0_rgba(16,185,129,0.5)]"
            : "inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-lg text-sm font-medium transition-all text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100 border border-transparent";
        return (
          <button
            key={tab.value}
            type="button"
            disabled={isDisabled}
            title={tab.tooltip}
            onClick={() => {
              if (!isDisabled) {
                playSfx("button-click-header");
                onValueChange(tab.value);
              }
            }}
            className={btnClass}
          >
            {tab.icon}
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}


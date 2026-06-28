import { playSfx } from "../../lib/sfx";

interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  sfx?: string | false;
}

export function Switch({ checked, onCheckedChange, sfx = "button-click-else" }: SwitchProps) {
  return (
    <button
      type="button"
      aria-pressed={checked}
      onClick={() => {
        if (sfx) playSfx(sfx);
        onCheckedChange(!checked);
      }}
      className={`relative h-6 w-11 rounded-full border transition-all duration-200 ${
        checked ? "border-emerald-500 bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.3)]" : "border-zinc-700 bg-zinc-800"
      }`}
    >
      <span
        className={`absolute top-0.5 h-5 w-5 rounded-full bg-zinc-100 transition-all duration-200 ${
          checked ? "left-5" : "left-0.5"
        }`}
      />
    </button>
  );
}


import { useEffect, useRef } from "react";

interface AudioMeterProps {
  level: number;
  bars?: number;
  className?: string;
}

export function AudioMeter({ level, bars = 20, className = "" }: AudioMeterProps) {
  const barRefs = useRef<(HTMLDivElement | null)[]>([]);
  const rafRef = useRef<number | undefined>(undefined);
  const levelRef = useRef(0);
  const displayRef = useRef<number[]>([]);
  const peakRef = useRef<number[]>([]);
  const peakHoldRef = useRef<number[]>([]);

  useEffect(() => {
    displayRef.current = new Array(bars).fill(0);
    peakRef.current = new Array(bars).fill(0);
    peakHoldRef.current = new Array(bars).fill(0);
  }, [bars]);

  useEffect(() => {
    levelRef.current = level;
  }, [level]);

  useEffect(() => {
    let lastTime = performance.now();

    const animate = (now: number) => {
      const dt = Math.min((now - lastTime) / 1000, 0.05);
      lastTime = now;

      const target = levelRef.current;
      const display = displayRef.current;

      // Smooth the incoming level: fast attack, slow release
      for (let i = 0; i < bars; i++) {
        const prev = display[i];
        const speed = target > prev ? 15 : 6;
        display[i] = prev + (target - prev) * Math.min(1, speed * dt);
      }

      const v = display[0]; // smoothed master level

      for (let i = 0; i < bars; i++) {
        const t = i / (bars - 1);
        // Per-bar variation driven by the real level
        const barWave =
          Math.sin(now * 0.008 + t * 4.5 + i * 0.7) * 0.12 +
          Math.sin(now * 0.013 + t * 7.0 - i * 0.3) * 0.08;
        const centerWeight = 1 - Math.abs(t - 0.35) * 0.5;
        let barLevel = v * centerWeight + barWave * v * 0.5;
        barLevel = Math.max(0, Math.min(1, barLevel));

        // Peak hold with decay
        if (barLevel > peakRef.current[i]) {
          peakRef.current[i] = barLevel;
          peakHoldRef.current[i] = 0.3;
        } else {
          peakHoldRef.current[i] -= dt;
          if (peakHoldRef.current[i] <= 0) {
            peakRef.current[i] = Math.max(0, peakRef.current[i] - dt * 1.2);
          }
        }

        const el = barRefs.current[i];
        if (el) {
          el.style.height = `${barLevel * 100}%`;
          const hue = 158 - i * 3;
          el.style.background =
            barLevel > 0.01
              ? `linear-gradient(to top, hsl(${hue} 75% ${30 + barLevel * 25}%), hsl(${hue + 25} 90% ${50 + barLevel * 20}%))`
              : "transparent";
          el.style.boxShadow = barLevel > 0.5 ? `0 0 6px hsl(${hue} 85% 55% / ${barLevel * 0.7})` : "none";
          el.style.opacity = `${0.6 + barLevel * 0.4}`;
        }
      }

      rafRef.current = requestAnimationFrame(animate);
    };

    rafRef.current = requestAnimationFrame(animate);
    return () => {
      if (rafRef.current !== undefined) cancelAnimationFrame(rafRef.current);
    };
  }, [bars]);

  return (
    <div className={`flex h-5 items-end gap-[2px] ${className}`}>
      {Array.from({ length: bars }, (_, i) => (
        <div
          key={i}
          ref={(el) => { barRefs.current[i] = el; }}
          className="w-[2px] min-w-[2px] flex-1 rounded-full"
          style={{ height: "0%", background: "transparent" }}
        />
      ))}
    </div>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import defEyeLogo from "./defEYE.png";
import {
  ArrowDown,
  ArrowUp,
  Brain,
  Camera,
  Film,
  GripVertical,
  Mic,
  MicOff,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Clock,
  Crosshair,
  Eye,
  EyeOff,
  FileImage,
  FolderOpen,
  HardDrive,
  Info,
  Layers,
  Lock,
  Loader2,
  Monitor,
  MonitorDown,
  Move,
  Play,
  Radar,
  RefreshCcw,
  Search,
  Shield,
  ShieldCheck,
  Square,
  StickyNote,
  Trash2,
  Unlock,
  Video,
  Volume2,
  VolumeX,
  XCircle,
} from "lucide-react";
import { Toaster, toast } from "sonner";
import { AudioMeter } from "./components/ui/AudioMeter";
import { api } from "./lib/tauri";
import { formatBytes, formatDuration, formatTimestamp } from "./lib/format";
import { playSfx, isSfxEnabled, setSfxEnabled } from "./lib/sfx";
import type {
  AnalysisResult,
  AudioDevice,
  AudioInputDevice,
  CaptureInfo,
  CaptureNotePayload,
  CaptureStats,
  DiskInfoPayload,
  FileCreatedPayload,
  HotkeySettings,
  HudCorner,
  IntegrityResult,
  MonitorInfo,
  MotionDetectedPayload,
  MotionStatusPayload,
  Settings,
  StatusPayload,
  TimelapseStatusPayload,
  VoiceAction,
  VoiceCommand,
  VoiceStatusPayload,
} from "./types";
import { Button } from "./components/ui/Button";
import { Card } from "./components/ui/Card";
import { Input } from "./components/ui/Input";
import { Switch } from "./components/ui/Switch";
import { Tabs } from "./components/ui/Tabs";
import { Textarea } from "./components/ui/Textarea";

type Tab = "camera" | "captures" | "voice" | "analysis" | "ai" | "about";
type ThemeName = "sentinel" | "amber" | "cosmotech";

const THEME_STORAGE_KEY = "defeye-theme";

const themes: { id: ThemeName; label: string; description: string }[] = [
  { id: "sentinel", label: "Sentinel", description: "Default emerald dark UI" },
  { id: "amber", label: "Amber Tactical", description: "Warm amber-on-charcoal tactical palette" },
  { id: "cosmotech", label: "CosmoTech™", description: "Cosmic cyan-violet living instrument" },
];

const defaultSettings: Settings = {
  camera_device: "",
  manual_camera_device: "",
  crf: 23,
  include_audio: true,
  screen_fps: 15,
  screen_crf: 23,
  output_dir: "",
  primary_monitor_id: "",
  hud_corner: "top_right",
  hud_minimal: false,
  motion_mode_enabled: false,
  motion_sensitivity: 50,
  motion_cooldown_seconds: 30,
  auto_record_on_motion: true,
  motion_triggers_screen: false,
  motion_post_record_seconds: 15,
  motion_min_record_seconds: 5,
  webcam_audio_device: "",
  webcam_audio_enabled: true,
  screen_audio_device: "",
  screen_audio_enabled: false,
  screen_capture_mode: "all_monitors",
  screen_monitor_id: "",
  recording_preset: "medium",
  max_recording_duration: 0,
  auto_restart_recording: true,
  multi_camera_devices: [],
  multi_camera_mode: "single",
  screenshot_region_mode: "full",
  custom_region: { x: 0, y: 0, width: 1920, height: 1080 },
  watermark_enabled: false,
  watermark_image_enabled: false,
  watermark_image_path: "",
  watermark_opacity: 0.5,
  watermark_scale: 0.1,
  watermark_position: "bottom_right",
  watermark_x: 10,
  watermark_y: 10,
  embed_metadata: false,
  integrity_check: false,
  ollama_enabled: false,
  ollama_endpoint: "http://localhost:11434",
  ollama_model: "qwen2.5vl:7b",
  auto_analysis_on_capture: false,
  ollama_temperature: 0.3,
  ollama_max_tokens: 1024,
  ollama_system_prompt: "You are defEYE, the unblinking AI sentinel in Deffy's ACU. Analyze security captures factually, concisely, and alert on anomalies/people/movement. Output structured JSON: {summary, tags: [], confidence: 0-1, key_observations}",
  watchdog_enabled: true,
  disk_threshold_mb: 1000,
  timelapse_interval_seconds: 10,
  timelapse_target: "screen" as const,
  voice_control_enabled: false,
  voice_audio_device: "",
  voice_wake_word: "",
  voice_confidence_threshold: 0.65,
  voice_model_path: "",
  voice_commands: [
    { phrase: "press start", action: "start_webcam" as VoiceAction },
    { phrase: "game over", action: "stop_recording" as VoiceAction },
    { phrase: "screenshot", action: "capture_primary" as VoiceAction },
    { phrase: "panorama", action: "capture_all_merged" as VoiceAction },
    { phrase: "enable radar", action: "toggle_motion" as VoiceAction },
    { phrase: "disable radar", action: "disable_motion" as VoiceAction },
    { phrase: "stealth mode", action: "toggle_stealth" as VoiceAction },
    { phrase: "co-op mode", action: "start_screen_recording" as VoiceAction },
    { phrase: "exit co-op", action: "stop_screen_recording" as VoiceAction },
    { phrase: "quit to menu", action: "stop_all_and_disable_motion" as VoiceAction },
    { phrase: "open inventory", action: "show_settings" as VoiceAction },
  ],
  voice_theme_id: "Gamer (Default)",
  voice_commands_custom: [],
  voice_commands_custom2: [],
  voice_auto_start: false,
  voice_feedback: true,
  system_tray_enabled: true,
  hotkeys: {
    start_webcam: "Shift+ArrowUp",
    stop_webcam: "Shift+ArrowDown",
    start_screen: "Ctrl+ArrowUp",
    stop_screen: "Ctrl+ArrowDown",
    capture_current: "Ctrl+ArrowLeft",
    capture_all_merged: "Ctrl+ArrowRight",
    toggle_motion_mode: "Ctrl+Shift+ArrowUp",
    cycle_camera_left: "Ctrl+Shift+ArrowLeft",
    cycle_camera_right: "Ctrl+Shift+ArrowRight",
    capture_region_selector: "Ctrl+Alt+ArrowUp",
    toggle_stealth: "Ctrl+Shift+ArrowDown",
    toggle_timelapse: "Ctrl+Alt+ArrowRight",
    kill_defeye: "Ctrl+Alt+ArrowDown",
  },
};

const windowLabel = getCurrentWindow().label;

export default function App() {
  if (windowLabel === "hud") {
    return <HudApp />;
  }

  if (windowLabel === "region_selector") {
    return <RegionSelector />;
  }

  return <SettingsApp />;
}

function CosmoTechBackground() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef(0);
  const particlesRef = useRef<{
    x: number; y: number; vx: number; vy: number;
    radius: number; hue: number; alpha: number;
  }[]>([]);
  const dprRef = useRef(1);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const resize = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      dprRef.current = window.devicePixelRatio || 1;
      canvas.width = w * dprRef.current;
      canvas.height = h * dprRef.current;
      canvas.style.width = w + "px";
      canvas.style.height = h + "px";
      ctx.setTransform(dprRef.current, 0, 0, dprRef.current, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    const PARTICLE_COUNT = 70;
    const MAX_LINK_DIST = 140;
    const particles = particlesRef.current;

    const spawnParticles = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      particles.length = 0;
      for (let i = 0; i < PARTICLE_COUNT; i++) {
        const isCyan = Math.random() > 0.4;
        particles.push({
          x: Math.random() * w,
          y: Math.random() * h,
          vx: (Math.random() - 0.5) * 0.25,
          vy: (Math.random() - 0.5) * 0.25,
          radius: Math.random() * 1.5 + 0.5,
          hue: isCyan ? 187 : 265,
          alpha: Math.random() * 0.4 + 0.15,
        });
      }
    };
    spawnParticles();

    const animate = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      ctx.clearRect(0, 0, w, h);

      for (let i = 0; i < particles.length; i++) {
        const p = particles[i];
        p.x += p.vx;
        p.y += p.vy;

        if (p.x < 0) p.x = w;
        if (p.x > w) p.x = 0;
        if (p.y < 0) p.y = h;
        if (p.y > h) p.y = 0;

        for (let j = i + 1; j < particles.length; j++) {
          const p2 = particles[j];
          const dx = p2.x - p.x;
          const dy = p2.y - p.y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < MAX_LINK_DIST) {
            const lineAlpha = (1 - dist / MAX_LINK_DIST) * 0.12 * Math.min(p.alpha, p2.alpha);
            const midHue = (p.hue + p2.hue) / 2;
            ctx.strokeStyle = `hsla(${midHue}, 80%, 60%, ${lineAlpha})`;
            ctx.lineWidth = 0.5;
            ctx.beginPath();
            ctx.moveTo(p.x, p.y);
            ctx.lineTo(p2.x, p2.y);
            ctx.stroke();
          }
        }

        const glowRadius = p.radius * 4;
        const gradient = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, glowRadius);
        gradient.addColorStop(0, `hsla(${p.hue}, 90%, 65%, ${p.alpha * 0.6})`);
        gradient.addColorStop(0.5, `hsla(${p.hue}, 80%, 55%, ${p.alpha * 0.15})`);
        gradient.addColorStop(1, `hsla(${p.hue}, 70%, 45%, 0)`);
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(p.x, p.y, glowRadius, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = `hsla(${p.hue}, 100%, 80%, ${p.alpha})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
        ctx.fill();
      }

      rafRef.current = requestAnimationFrame(animate);
    };

    rafRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(rafRef.current);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <div className="cosmotech-bg">
      <canvas ref={canvasRef} />
    </div>
  );
}

function AmberBackground() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef(0);
  const particlesRef = useRef<{
    x: number; y: number; vx: number; vy: number;
    radius: number; alpha: number; life: number; maxLife: number;
  }[]>([]);
  const sweepRef = useRef(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = window.innerWidth;
      const h = window.innerHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = w + "px";
      canvas.style.height = h + "px";
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    const MAX_PARTICLES = 50;
    const particles = particlesRef.current;

    const spawnEmber = (w: number, h: number) => {
      if (particles.length >= MAX_PARTICLES) return;
      particles.push({
        x: Math.random() * w,
        y: h + 10,
        vx: (Math.random() - 0.5) * 0.15,
        vy: -(Math.random() * 0.4 + 0.2),
        radius: Math.random() * 1.2 + 0.4,
        alpha: 0,
        life: 0,
        maxLife: Math.random() * 300 + 200,
      });
    };

    const animate = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      ctx.clearRect(0, 0, w, h);

      // Radar sweep — rotating amber arc from center-bottom
      const cx = w / 2;
      const cy = h;
      const maxRadius = Math.sqrt(w * w + h * h) / 2 + 100;
      const sweepAngle = sweepRef.current;
      sweepRef.current += 0.006;

      const sweepGradient = ctx.createConicGradient(
        sweepAngle - 0.4, cx, cy,
      );
      sweepGradient.addColorStop(0, "rgba(245, 158, 11, 0)");
      sweepGradient.addColorStop(0.15, "rgba(245, 158, 11, 0.015)");
      sweepGradient.addColorStop(0.25, "rgba(245, 158, 11, 0.04)");
      sweepGradient.addColorStop(0.3, "rgba(245, 158, 11, 0.015)");
      sweepGradient.addColorStop(0.35, "rgba(245, 158, 11, 0)");
      sweepGradient.addColorStop(1, "rgba(245, 158, 11, 0)");
      ctx.fillStyle = sweepGradient;
      ctx.fillRect(0, 0, w, h);

      // Concentric range rings
      ctx.strokeStyle = "rgba(245, 158, 11, 0.025)";
      ctx.lineWidth = 1;
      for (let r = 100; r < maxRadius; r += 120) {
        ctx.beginPath();
        ctx.arc(cx, cy, r, Math.PI, Math.PI * 2);
        ctx.stroke();
      }

      // Spawn embers
      if (Math.random() < 0.4) spawnEmber(w, h);

      // Update and draw embers
      for (let i = particles.length - 1; i >= 0; i--) {
        const p = particles[i];
        p.x += p.vx;
        p.y += p.vy;
        p.vy *= 0.998;
        p.vx += (Math.random() - 0.5) * 0.02;
        p.life++;

        const lifeRatio = p.life / p.maxLife;
        if (lifeRatio < 0.15) {
          p.alpha = lifeRatio / 0.15 * 0.5;
        } else if (lifeRatio > 0.7) {
          p.alpha = (1 - lifeRatio) / 0.3 * 0.5;
        } else {
          p.alpha = 0.5;
        }

        if (p.life >= p.maxLife || p.y < -20) {
          particles.splice(i, 1);
          continue;
        }

        const glowR = p.radius * 5;
        const grad = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, glowR);
        grad.addColorStop(0, `rgba(251, 191, 36, ${p.alpha * 0.5})`);
        grad.addColorStop(0.4, `rgba(245, 158, 11, ${p.alpha * 0.15})`);
        grad.addColorStop(1, "rgba(217, 119, 6, 0)");
        ctx.fillStyle = grad;
        ctx.beginPath();
        ctx.arc(p.x, p.y, glowR, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = `rgba(253, 230, 138, ${p.alpha})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
        ctx.fill();
      }

      rafRef.current = requestAnimationFrame(animate);
    };

    rafRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(rafRef.current);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <div className="amber-bg">
      <canvas ref={canvasRef} />
    </div>
  );
}

function SettingsApp() {
  const [activeTab, setActiveTab] = useState<Tab>("camera");
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [isRecording, setIsRecording] = useState(false);
  const [recordingKind, setRecordingKind] = useState("idle");
  const [webcamActive, setWebcamActive] = useState(false);
  const [screenActive, setScreenActive] = useState(false);
  const [multiActive, setMultiActive] = useState(false);
  const [finalizing, setFinalizing] = useState(false);
  const [captures, setCaptures] = useState<CaptureInfo[]>([]);
  const [cameraDevices, setCameraDevices] = useState<string[]>([]);
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [selectedCapture, setSelectedCapture] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [motionStatus, setMotionStatus] = useState<MotionStatusPayload>({
    motion_mode_enabled: false,
    motion_active: false,
    last_detection: null,
  });
  const [diskInfo, setDiskInfo] = useState<DiskInfoPayload | null>(null);
  const [timelapseStatus, setTimelapseStatus] = useState<TimelapseStatusPayload>({
    active: false,
    interval_seconds: 0,
    target: "screen",
    last_capture: null,
  });
  const [recordingDuration, setRecordingDuration] = useState<number | null>(null);
  const [voiceActive, setVoiceActive] = useState(false);
  const [webcamAudioLevel, setWebcamAudioLevel] = useState(0);
  const [screenAudioLevel, setScreenAudioLevel] = useState(0);
  const [sfxOn, setSfxOn] = useState(isSfxEnabled());
  const [theme, setTheme] = useState<ThemeName>(() => {
    try {
      const saved = localStorage.getItem(THEME_STORAGE_KEY);
      if (saved === "sentinel" || saved === "amber" || saved === "cosmotech") return saved;
    } catch { /* ignore */ }
    return "sentinel";
  });
  const prevRecordingRef = useRef<boolean | null>(null);
  const prevRecordingKindRef = useRef<string>("idle");

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    try { localStorage.setItem(THEME_STORAGE_KEY, theme); } catch { /* ignore */ }
  }, [theme]);

  const refreshCaptures = useCallback(async () => {
    const files = await api.listRecentCaptures();
    setCaptures(files);
    setSelectedCapture((current) => current ?? files[0]?.path ?? null);
  }, []);

  const refreshDevices = useCallback(async () => {
    const devices = await api.listCameras();
    setCameraDevices(devices);
  }, []);

  const refreshAudioDevices = useCallback(async () => {
    const devices = await api.listAudioDevices();
    setAudioDevices(devices);
  }, []);

  const refreshMonitors = useCallback(async () => {
    const items = await api.listMonitors();
    setMonitors(items);
  }, []);

  useEffect(() => {
    void api
      .getSettings()
      .then(setSettings)
      .catch((error: unknown) => toast.error(String(error)));
    void api
      .getStatus()
      .then((status) => {
        setIsRecording(status.is_recording);
        setRecordingKind(status.recording_kind);
        setWebcamActive(status.webcam_active);
        setScreenActive(status.screen_active);
        setMultiActive(status.multi_active);
        setFinalizing(status.finalizing);
        prevRecordingRef.current = status.is_recording;
        prevRecordingKindRef.current = status.recording_kind;
      })
      .catch((error: unknown) => toast.error(String(error)));
    void refreshCaptures().catch((error: unknown) => toast.error(String(error)));
    void refreshDevices().catch(() => undefined);
    void refreshAudioDevices().catch(() => undefined);
    void refreshMonitors().catch(() => undefined);

    const unlistenStatus = listen<StatusPayload>("status-updated", (event) => {
      if (prevRecordingRef.current !== null) {
        if (!prevRecordingRef.current && event.payload.is_recording) {
          playSfx(event.payload.recording_kind === "screen" ? "start-rec-screen" : "start-rec-webcam");
        } else if (prevRecordingRef.current && !event.payload.is_recording) {
          playSfx(prevRecordingKindRef.current === "screen" ? "stop-rec-screen" : "stop-rec-webcam");
        }
      }
      prevRecordingRef.current = event.payload.is_recording;
      prevRecordingKindRef.current = event.payload.recording_kind;
      setIsRecording(event.payload.is_recording);
      setRecordingKind(event.payload.recording_kind);
      setWebcamActive(event.payload.webcam_active);
      setScreenActive(event.payload.screen_active);
      setMultiActive(event.payload.multi_active);
      setFinalizing(event.payload.finalizing);
    });
    const unlistenFile = listen<FileCreatedPayload>("file-created", (event) => {
      if (!event.payload.path.includes("timelapse")) {
        toast.success(`Capture saved: ${event.payload.path}`);
      }
      void refreshCaptures().catch((error: unknown) => toast.error(String(error)));
    });
    const unlistenCaptureToast = listen<[string, string]>("capture-toast", (event) => {
      const [kind, path] = event.payload;
      const label = kind === "primary" ? "Primary screen captured" : "All screens captured";
      const filename = path.split(/[\\/]/).pop() ?? path;
      toast.success(`${label}: ${filename}`);
      playSfx(kind === "primary" ? "capture-primary" : "capture-all");
      void refreshCaptures().catch(() => undefined);
    });
    const unlistenError = listen<string>("defeye-error", (event) => {
      toast.error(event.payload);
    });
    const unlistenMotion = listen<MotionDetectedPayload>("motion-detected", (event) => {
      toast.warning(`Motion detected — Recording started`, {
        description: event.payload.timestamp,
      });
      void refreshCaptures().catch(() => undefined);
    });
    const unlistenMotionStatus = listen<MotionStatusPayload>("motion-status-updated", (event) => {
      setMotionStatus(event.payload);
      setSettings((prev) => {
        if (prev.motion_mode_enabled !== event.payload.motion_mode_enabled) {
          return { ...prev, motion_mode_enabled: event.payload.motion_mode_enabled };
        }
        return prev;
      });
    });
    const unlistenMotionToggled = listen<boolean>("motion-toggled", (enabled) => {
      toast.info(enabled.payload ? "Sentinel Motion Mode ENABLED" : "Sentinel Motion Mode DISABLED");
      if (enabled.payload) playSfx("sentinel");
    });
    const unlistenCameraCycled = listen<string>("camera-cycled", (event) => {
      setSettings((prev) => ({ ...prev, camera_device: event.payload }));
    });
    const unlistenRegionSelected = listen<{ x: number; y: number; width: number; height: number }>("region-selected", (event) => {
      setSettings((prev) => {
        const next = { ...prev, custom_region: event.payload, screenshot_region_mode: "custom" as const };
        void api.updateSettings(next);
        return next;
      });
      toast.success(`Region selected: ${event.payload.width} x ${event.payload.height}`);
    });
    const unlistenScreenMonitorSwitched = listen<string>("screen-monitor-switched", (event) => {
      void api.getSettings().then((fresh) => setSettings(fresh));
      toast.success(`Screen target switched to ${event.payload}`);
    });
    const unlistenTimelapse = listen<TimelapseStatusPayload>("timelapse-status", (event) => {
      setTimelapseStatus(event.payload);
    });
    const unlistenTimelapseStarted = listen<TimelapseStatusPayload>("timelapse-started", (event) => {
      setTimelapseStatus(event.payload);
      toast.success("Time-Lapse ENGAGED", {
        description: `Interval: ${event.payload.interval_seconds}s · Target: ${event.payload.target}`,
        duration: 4000,
      });
      playSfx("time-lapse");
    });
    const unlistenTimelapseStopped = listen<TimelapseStatusPayload>("timelapse-stopped", (event) => {
      setTimelapseStatus(event.payload);
      toast.info("Time-Lapse DISENGAGED", {
        description: event.payload.last_capture
          ? `Last capture: ${event.payload.last_capture}`
          : "No captures were taken",
        duration: 4000,
      });
    });
    const unlistenVoiceStatus = listen<VoiceStatusPayload>("voice-status", (event) => {
      setVoiceActive(event.payload.active);
    });
    const unlistenAudioLevel = listen<{ webcam: number; screen: number }>("audio-level", (event) => {
      setWebcamAudioLevel(event.payload.webcam);
      setScreenAudioLevel(event.payload.screen);
    });
    const unlistenSettingsUpdated = listen<Settings>("settings-updated", (event) => {
      setSettings(event.payload);
    });

    void api.getTimelapseStatus().then(setTimelapseStatus).catch(() => undefined);
    void api.getDiskInfo().then(setDiskInfo).catch(() => undefined);
    void api.getVoiceStatus().then((s) => setVoiceActive(s.active)).catch(() => undefined);

    const diskInterval = setInterval(() => {
      void api.getDiskInfo().then(setDiskInfo).catch(() => undefined);
    }, 10000);

    const durationInterval = setInterval(() => {
      void api.getRecordingDuration().then(setRecordingDuration).catch(() => undefined);
    }, 1000);

    void api.getMotionStatus().then(setMotionStatus).catch(() => undefined);

    return () => {
      void unlistenStatus.then((dispose) => dispose());
      void unlistenFile.then((dispose) => dispose());
      void unlistenCaptureToast.then((dispose) => dispose());
      void unlistenError.then((dispose) => dispose());
      void unlistenMotion.then((dispose) => dispose());
      void unlistenMotionStatus.then((dispose) => dispose());
      void unlistenMotionToggled.then((dispose) => dispose());
      void unlistenCameraCycled.then((dispose) => dispose());
      void unlistenRegionSelected.then((dispose) => dispose());
      void unlistenScreenMonitorSwitched.then((dispose) => dispose());
      void unlistenTimelapse.then((dispose) => dispose());
      void unlistenTimelapseStarted.then((dispose) => dispose());
      void unlistenTimelapseStopped.then((dispose) => dispose());
      void unlistenVoiceStatus.then((dispose) => dispose());
      void unlistenAudioLevel.then((dispose) => dispose());
      void unlistenSettingsUpdated.then((dispose) => dispose());
      clearInterval(diskInterval);
      clearInterval(durationInterval);
    };
  }, [refreshCaptures, refreshDevices, refreshAudioDevices, refreshMonitors]);

  // Start/stop real audio level monitoring based on audio settings
  useEffect(() => {
    const webcamOn = settings.webcam_audio_enabled;
    const screenOn = settings.screen_audio_enabled;
    if (!webcamOn && !screenOn) {
      void api.stopAudioLevelMonitor().catch(() => undefined);
      setWebcamAudioLevel(0);
      setScreenAudioLevel(0);
      return;
    }
    const webcamDev = webcamOn ? settings.webcam_audio_device : "";
    const screenDev = screenOn ? settings.screen_audio_device : "";
    void api.startAudioLevelMonitor(webcamDev, screenDev).catch((error: unknown) => {
      console.error("Failed to start audio level monitor:", error);
    });
    return () => {
      void api.stopAudioLevelMonitor().catch(() => undefined);
    };
  }, [settings.webcam_audio_enabled, settings.webcam_audio_device, settings.screen_audio_enabled, settings.screen_audio_device]);

  const saveSettings = useCallback(async (next: Settings) => {
    const wasEnabled = settings.ollama_enabled;
    const nowEnabled = next.ollama_enabled;
    setSettings(next);
    await api.updateSettings(next);
    if (!wasEnabled && nowEnabled) {
      toast.success("Ollama enabled — Analysis tab is now available");
    }
    if (wasEnabled && !nowEnabled) {
      toast.warning("Ollama disabled — Analysis tab is now locked");
      if (activeTab === "analysis") {
        setActiveTab("ai");
      }
    }
  }, [settings.ollama_enabled, activeTab]);

  const runAction = useCallback(
    async (key: string, action: () => Promise<string | void>, success: string) => {
      try {
        setBusyAction(key);
        const result = await action();
        toast.success(result ? `${success}: ${result}` : success);
        await refreshCaptures();
      } catch (error) {
        toast.error(String(error));
      } finally {
        setBusyAction(null);
      }
    },
    [refreshCaptures],
  );

  const lastCapture = captures[0] ?? null;
  const selectedFile = selectedCapture ?? lastCapture?.path ?? null;

  const tabs = useMemo(
    () => [
      { value: "camera" as const, label: "Camera", icon: <Camera className="h-4 w-4" /> },
      { value: "captures" as const, label: "Captures", icon: <FileImage className="h-4 w-4" /> },
      { value: "voice" as const, label: "Voice", icon: <Mic className="h-4 w-4" /> },
      { value: "ai" as const, label: "AI", icon: <Brain className="h-4 w-4" /> },
      {
        value: "analysis" as const,
        label: "Analysis",
        icon: <Search className="h-4 w-4" />,
        disabled: !settings.ollama_enabled,
        tooltip: settings.ollama_enabled
          ? undefined
          : "Enable Ollama in the AI tab to use Analysis",
      },
      { value: "about" as const, label: "About", icon: <Info className="h-4 w-4" /> },
    ],
    [settings.ollama_enabled],
  );

  return (
    <div className={`dark min-h-screen bg-zinc-950 text-zinc-200 sentinel-grid-bg ${isRecording ? "sentinel-recording-glow" : ""}`}>
      {theme === "cosmotech" && <CosmoTechBackground />}
      {theme === "amber" && <AmberBackground />}
      <Toaster richColors position="bottom-right" theme="dark" toastOptions={{ style: { borderLeft: "2px solid rgb(16 185 129)" } }} />
      <main className="relative z-10 mx-auto flex h-screen max-w-6xl flex-col px-6 py-5">
        <header className="mb-3 flex items-center justify-between border-b border-zinc-800/80 pb-3">
          <div className="flex items-center gap-3">
            <div
              onMouseDown={(e) => {
                if (e.button === 0) {
                  void getCurrentWindow().startDragging();
                }
              }}
              className="flex cursor-grab items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-zinc-500 transition-colors hover:border-zinc-700 hover:text-zinc-300 active:cursor-grabbing"
              title="Drag to move window"
            >
              <Move className="h-4 w-4" />
            </div>
            <div>
              <div className="flex items-center gap-3">
                <img src={defEyeLogo} alt="defEYE" className="sentinel-logo h-14 w-14 rounded-lg ring-1 ring-emerald-500/20" />
                <div className="flex flex-col">
                  <h1 className="defeye-title text-3xl text-zinc-50 leading-none">
                    <span className="font-medium text-zinc-400">def</span><span className="font-black text-emerald-400">EYE</span>
                  </h1>
                  <span
                    className="defeye-credit text-xs text-zinc-500 tracking-[0.5px] mt-1 hover:text-emerald-400"
                    title="Visit deffy.me"
                    onMouseEnter={() => toast("Click to visit deffy.me", { duration: 2000, style: { boxShadow: "0 0 12px rgba(204, 0, 17, 0.5), 0 0 4px rgba(204, 0, 17, 0.3)", border: "1px solid rgba(204, 0, 17, 0.4)" } })}
                    onClick={() => { playSfx("button-click-else"); void api.openUrl("https://deffy.me").catch((error: unknown) => toast.error(String(error))) }}
                  >
                    Deffy Urz
                  </span>
                </div>
              </div>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <StatusPill isRecording={isRecording} recordingKind={recordingKind} finalizing={finalizing} />
            {recordingDuration != null && isRecording && (
              <span className="flex items-center gap-1.5 rounded-lg bg-zinc-900 border border-zinc-800 px-2.5 py-1 text-xs font-mono text-emerald-300">
                <Clock className="h-3 w-3" />
                {Math.floor(recordingDuration / 60)}:{String(recordingDuration % 60).padStart(2, "0")}
              </span>
            )}
            {diskInfo && (
              <div className={`flex items-center gap-2 rounded-lg border px-2.5 py-1 ${diskInfo.warning ? "border-red-500/40 bg-red-950/40" : "border-zinc-800 bg-zinc-900"}`}>
                <HardDrive className={`h-3 w-3 ${diskInfo.warning ? "text-red-400" : "text-emerald-400"}`} />
                <span className={`text-xs font-mono ${diskInfo.warning ? "text-red-300" : "text-zinc-400"}`}>
                  {diskInfo.free_mb >= 1024 ? `${(diskInfo.free_mb / 1024).toFixed(1)} GB` : `${diskInfo.free_mb} MB`}
                </span>
                <div className="h-1 w-12 overflow-hidden rounded-full bg-zinc-800">
                  <div
                    className={`h-full rounded-full transition-all duration-300 ${diskInfo.warning ? "bg-red-500" : "bg-emerald-500"}`}
                    style={{ width: `${Math.min(100, diskInfo.total_bytes > 0 ? (1 - diskInfo.free_mb / (diskInfo.total_bytes / (1024 * 1024))) * 100 : 0)}%` }}
                  />
                </div>
              </div>
            )}
            <button
              onClick={() => { setSfxOn((prev) => { const next = !prev; setSfxEnabled(next); playSfx("sfx-toggle"); return next; }); }}
              className={`flex h-8 w-8 items-center justify-center rounded-lg border transition-all duration-200 ${sfxOn ? "border-emerald-500/40 bg-emerald-900/40 text-emerald-300" : "border-zinc-800 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300 hover:scale-105"}`}
              title="Toggle Sound Effects"
            >
              {sfxOn ? <Volume2 className="h-4 w-4" /> : <VolumeX className="h-4 w-4" />}
            </button>
            <button
              onClick={() => { playSfx("button-click-else"); void api.toggleVoiceControl().then((active: boolean) => { setVoiceActive(active); void api.getSettings().then(setSettings); toast.info(active ? "Voice control ENGAGED" : "Voice control DISENGAGED"); }).catch((error: unknown) => toast.error(String(error))) }}
              className={`flex h-8 w-8 items-center justify-center rounded-lg border transition-all duration-200 ${voiceActive ? "border-emerald-500/40 bg-emerald-900/40 text-emerald-300" : "border-zinc-800 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300 hover:scale-105"}`}
              title="Toggle Voice Control"
            >
              {voiceActive ? <Mic className="h-4 w-4" /> : <MicOff className="h-4 w-4" />}
            </button>
              <button
                onClick={() => { void api.toggleStealthMode().catch((error: unknown) => toast.error(String(error))) }}
                className="flex h-8 w-8 items-center justify-center rounded-lg border border-zinc-800 text-zinc-500 transition-all duration-200 hover:bg-zinc-800 hover:text-zinc-300 hover:scale-105"
                title={`Toggle Stealth (${settings.hotkeys.toggle_stealth})`}
              >
                <EyeOff className="h-4 w-4" />
              </button>
          </div>
        </header>

        <Tabs value={activeTab} onValueChange={setActiveTab} tabs={tabs} />

        <section className="min-h-0 flex-1 overflow-hidden py-5">
          {activeTab === "camera" && (
            <CameraTab
              settings={settings}
              isRecording={isRecording}
              recordingKind={recordingKind}
              webcamActive={webcamActive}
              screenActive={screenActive}
              multiActive={multiActive}
              finalizing={finalizing}
              busyAction={busyAction}
              cameraDevices={cameraDevices}
              audioDevices={audioDevices}
              monitors={monitors}
              refreshDevices={refreshDevices}
              refreshAudioDevices={refreshAudioDevices}
              refreshMonitors={refreshMonitors}
              saveSettings={saveSettings}
              runAction={runAction}
              motionStatus={motionStatus}
              timelapseStatus={timelapseStatus}
              diskInfo={diskInfo}
              webcamAudioLevel={webcamAudioLevel}
              screenAudioLevel={screenAudioLevel}
            />
          )}
          {activeTab === "captures" && (
            <CapturesTab
              captures={captures}
              outputDir={settings.output_dir}
              refreshCaptures={refreshCaptures}
              setSelectedCapture={setSelectedCapture}
              timelapseStatus={timelapseStatus}
              settings={settings}
              saveSettings={saveSettings}
            />
          )}
          {activeTab === "voice" && (
            <VoiceCommandsTab
              settings={settings}
              saveSettings={saveSettings}
              voiceActive={voiceActive}
            />
          )}
          {activeTab === "analysis" && (
            <AnalysisTab
              captures={captures}
              selectedFile={selectedFile}
              setSelectedCapture={setSelectedCapture}
              prompt={prompt}
              setPrompt={setPrompt}
              analysis={analysis}
              setAnalysis={setAnalysis}
              busyAction={busyAction}
              setBusyAction={setBusyAction}
            />
          )}
          {activeTab === "ai" && (
            <AiTab settings={settings} saveSettings={saveSettings} />
          )}
          {activeTab === "about" && <AboutTab outputDir={settings.output_dir} settings={settings} setSettings={setSettings} theme={theme} setTheme={setTheme} />}
        </section>
      </main>
    </div>
  );
}

function HudApp() {
  const [status, setStatus] = useState<StatusPayload>({
    is_recording: false,
    recording_kind: "idle",
    webcam_active: false,
    screen_active: false,
    multi_active: false,
    finalizing: false,
  });
  const [motionActive, setMotionActive] = useState(false);
  const [motionFlash, setMotionFlash] = useState(false);
  const [minimal, setMinimal] = useState(false);
  const [voiceActive, setVoiceActive] = useState(false);

  useEffect(() => {
    document.documentElement.classList.add("hud-html");
    void api.getStatus().then(setStatus).catch(() => undefined);
    void api.getMotionStatus().then((m) => setMotionActive(m.motion_active)).catch(() => undefined);
    void api.getSettings().then((s) => setMinimal(s.hud_minimal)).catch(() => undefined);
    void api.getVoiceStatus().then((s) => setVoiceActive(s.active)).catch(() => undefined);
    const unlistenStatus = listen<StatusPayload>("status-updated", (event) => {
      setStatus(event.payload);
    });
    const unlistenMotionStatus = listen<MotionStatusPayload>("motion-status-updated", (event) => {
      setMotionActive(event.payload.motion_active);
    });
    const unlistenMotionDetected = listen<MotionDetectedPayload>("motion-detected", () => {
      setMotionFlash(true);
      setTimeout(() => setMotionFlash(false), 2000);
    });
    const unlistenSettings = listen<Settings>("settings-updated", (event) => {
      setMinimal(event.payload.hud_minimal);
    });
    const unlistenVoiceStatus = listen<VoiceStatusPayload>("voice-status", (event) => {
      setVoiceActive(event.payload.active);
    });

    return () => {
      document.documentElement.classList.remove("hud-html");
      void unlistenStatus.then((dispose) => dispose());
      void unlistenMotionStatus.then((dispose) => dispose());
      void unlistenMotionDetected.then((dispose) => dispose());
      void unlistenSettings.then((dispose) => dispose());
      void unlistenVoiceStatus.then((dispose) => dispose());
    };
  }, []);

  const label = motionFlash
    ? "MOTION"
    : status.is_recording
      ? status.recording_kind === "webcam"
        ? "CAM"
        : status.recording_kind === "screen"
          ? "SCR"
          : status.recording_kind === "mixed"
            ? "MIX"
            : status.recording_kind.toUpperCase()
      : status.finalizing
        ? "SAVE"
        : motionActive
          ? "SCAN"
          : voiceActive
            ? "VOICE"
            : "IDLE";

  const dotColor = motionFlash
    ? "bg-amber-400"
    : status.is_recording
      ? "bg-red-500"
      : status.finalizing
        ? "bg-amber-400"
        : motionActive
          ? "bg-amber-500"
          : voiceActive
            ? "bg-sky-400"
            : "bg-emerald-500";

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent">
      <div className={`hud-shell flex h-7 items-center justify-center rounded-md border shadow-lg transition-all duration-200 ${
        minimal ? "w-7 px-0" : "w-[118px] gap-2 px-2"
      } text-[11px] font-semibold ${
        motionFlash
          ? "border-amber-500/60 bg-amber-950/60 text-amber-200"
          : "border-zinc-700/50 bg-zinc-950/45 text-zinc-300"
      }`}>
        <span className={`rounded-full ${dotColor} ${(motionActive && !motionFlash) || voiceActive ? "animate-pulse" : ""} ${minimal ? "h-2.5 w-2.5" : "h-2 w-2"}`} />
        {!minimal && <span className="max-w-[78px] truncate tracking-normal">{label}</span>}
      </div>
    </div>
  );
}

function StatusPill({ isRecording, recordingKind, finalizing = false }: { isRecording: boolean; recordingKind: string; finalizing?: boolean }) {
  const active = isRecording || finalizing;
  const label = finalizing && !isRecording ? "SAVING" : isRecording ? `REC ${recordingKind.toUpperCase()}` : "IDLE";

  return (
    <div
      className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-semibold transition-all duration-200 ${
        isRecording
          ? "border-red-500/60 bg-red-500/10 text-red-300 sentinel-pill-recording"
          : finalizing
            ? "border-amber-500/60 bg-amber-500/10 text-amber-300"
            : "border-zinc-800 bg-zinc-900 text-zinc-400"
      }`}
    >
      <span className={`h-2 w-2 rounded-full ${isRecording ? "bg-red-400 animate-pulse" : finalizing ? "bg-amber-400 animate-pulse" : "bg-emerald-500"}`} />
      {active ? label : "IDLE"}
    </div>
  );
}

interface CameraTabProps {
  settings: Settings;
  isRecording: boolean;
  recordingKind: string;
  webcamActive: boolean;
  screenActive: boolean;
  multiActive: boolean;
  finalizing: boolean;
  busyAction: string | null;
  cameraDevices: string[];
  audioDevices: AudioDevice[];
  monitors: MonitorInfo[];
  refreshDevices: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshMonitors: () => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  runAction: (key: string, action: () => Promise<string | void>, success: string) => Promise<void>;
  motionStatus: MotionStatusPayload;
  timelapseStatus: TimelapseStatusPayload;
  diskInfo: DiskInfoPayload | null;
  webcamAudioLevel: number;
  screenAudioLevel: number;
}

function CameraTab({
  settings,
  isRecording,
  recordingKind,
  webcamActive,
  screenActive,
  multiActive,
  finalizing,
  busyAction,
  cameraDevices,
  audioDevices,
  monitors,
  refreshDevices,
  refreshAudioDevices,
  refreshMonitors,
  saveSettings,
  runAction,
  motionStatus,
  timelapseStatus,
  diskInfo,
  webcamAudioLevel,
  screenAudioLevel,
}: CameraTabProps) {
  const updateField = async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    await saveSettings({ ...settings, [key]: value });
  };

  const changeFolder = async () => {
    const selected = await openDialog({ directory: true, multiple: false, title: "Choose defEYE output folder" });
    if (typeof selected === "string") {
      await updateField("output_dir", selected);
      toast.success("Output folder updated");
    }
  };

  const refreshCameraList = async () => {
    try {
      await refreshDevices();
      toast.success("Device list refreshed");
    } catch (error) {
      toast.error(String(error));
    }
  };

  const refreshMonitorList = async () => {
    try {
      await refreshMonitors();
      toast.success("Monitor list refreshed");
    } catch (error) {
      toast.error(String(error));
    }
  };

  const refreshAudioList = async () => {
    try {
      await refreshAudioDevices();
      toast.success("Audio device list refreshed");
    } catch (error) {
      toast.error(String(error));
    }
  };

  const [previewActive, setPreviewActive] = useState(false);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [previewNonce, setPreviewNonce] = useState(0);

  const startPreview = async () => {
    try {
      const path = await api.startCameraPreview();
      setPreviewActive(true);
      setPreviewSrc(convertFileSrc(path));
      setPreviewNonce((n) => n + 1);
    } catch (error) {
      toast.error(String(error));
    }
  };

  const stopPreview = async () => {
    try {
      await api.stopCameraPreview();
    } catch {
      // ignore
    }
    setPreviewActive(false);
    setPreviewSrc(null);
  };

  useEffect(() => {
    if (!previewActive || !previewSrc) return;
    const interval = setInterval(() => {
      setPreviewNonce((n) => n + 1);
    }, 1500);
    return () => clearInterval(interval);
  }, [previewActive, previewSrc]);

  useEffect(() => {
    return () => {
      void api.stopCameraPreview().catch(() => undefined);
    };
  }, []);

  const toggleMultiCameraDevice = async (device: string) => {
    const current = settings.multi_camera_devices;
    const next = current.includes(device)
      ? current.filter((d) => d !== device)
      : [...current, device];
    await updateField("multi_camera_devices", next);
  };

  return (
    <div className="grid h-full grid-cols-[1.08fr_0.92fr] gap-5">
      <Card className="min-h-0 overflow-auto p-5">
        <div className="mb-5 flex items-center">
          <h2 className="text-base font-semibold text-zinc-50">Capture Settings</h2>
        </div>

        <div className="space-y-6">
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Detected camera</span>
            <div className="flex gap-2">
              <select
                value={settings.camera_device}
                onChange={(event) =>
                  updateField("camera_device", event.target.value).catch((error: unknown) => toast.error(String(error)))
                }
                className="h-9 min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
              >
                <option value="">Select camera</option>
                {cameraDevices.map((device) => (
                  <option key={device} value={device}>
                    {device}
                  </option>
                ))}
              </select>
              <Button onClick={() => void refreshCameraList()}>
                <RefreshCcw className="h-4 w-4" />
                Refresh
              </Button>
              {previewActive ? (
                <Button
                  variant="primary"
                  disabled={busyAction === "preview_stop"}
                  onClick={() => void stopPreview()}
                >
                  {busyAction === "preview_stop" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
                  Deactivate
                </Button>
              ) : (
                <Button
                  variant="primary"
                  disabled={!settings.camera_device || busyAction === "preview_start"}
                  onClick={() => void startPreview()}
                >
                  {busyAction === "preview_start" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Camera className="h-4 w-4" />}
                  Activate
                </Button>
              )}
            </div>
          </label>

          {previewActive && previewSrc && (
            <div className="overflow-hidden rounded-lg border border-zinc-800 bg-black">
              <img
                src={`${previewSrc}?t=${previewNonce}`}
                alt="Camera preview"
                className="mx-auto max-h-64 w-auto"
              />
            </div>
          )}

          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Manual ffmpeg device override</span>
            <Input
              value={settings.manual_camera_device}
              onChange={(event) =>
                updateField("manual_camera_device", event.target.value).catch((error: unknown) => toast.error(String(error)))
              }
              placeholder='video="Integrated Camera"'
            />
          </label>

          {/* Recording quality preset */}
          <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
            <span className="mb-2 block text-xs font-semibold uppercase tracking-wider text-zinc-400">Quality Preset</span>
            <div className="flex gap-2">
              {(["ultra", "high", "medium", "low", "custom"] as const).map((p) => (
                <button
                  key={p}
                  onClick={() => { playSfx("button-click-else"); void updateField("recording_preset", p).catch((error: unknown) => toast.error(String(error))) }}
                  className={`flex-1 rounded-md border px-2 py-1.5 text-xs font-medium capitalize transition ${
                    settings.recording_preset === p
                      ? "border-emerald-500 bg-emerald-500/10 text-emerald-300"
                      : "border-zinc-800 bg-zinc-950 text-zinc-400 hover:border-zinc-700"
                  }`}
                >
                  {p}
                </button>
              ))}
            </div>

            {/* Animated CRF slider — only visible when Custom preset is selected */}
            <div
              className={`overflow-hidden transition-all duration-300 ease-in-out ${
                settings.recording_preset === "custom"
                  ? "mt-3 max-h-40 opacity-100"
                  : "mt-0 max-h-0 opacity-0"
              }`}
            >
              <label className="block">
                <div className="mb-2 flex items-center justify-between text-sm">
                  <span className="text-zinc-400">CRF quality</span>
                  <span className="font-mono text-emerald-300">{settings.crf}</span>
                </div>
                <input
                  type="range"
                  min={18}
                  max={32}
                  step={1}
                  value={settings.crf}
                  onChange={(event) =>
                    updateField("crf", Number(event.target.value)).catch((error: unknown) => toast.error(String(error)))
                  }
                  className="w-full accent-emerald-500"
                />
                <div className="mt-1 flex justify-between text-xs text-zinc-600">
                  <span>18</span>
                  <span>32</span>
                </div>
              </label>
            </div>

            <p className="mt-2 text-xs text-zinc-500">
              {settings.recording_preset === "custom"
                ? ""
                : settings.recording_preset === "medium"
                  ? "Uses stored CRF values. Switch to Custom to adjust."
                  : "Overrides CRF for all recordings. Ultra=18, High=20, Low=28."}
            </p>
          </div>

          {/* Auto-stop max duration */}
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Max recording duration (seconds, 0 = unlimited)</span>
            <Input
              type="number"
              min={0}
              value={settings.max_recording_duration}
              onChange={(event) =>
                updateField("max_recording_duration", Math.max(0, Number(event.target.value))).catch((error: unknown) => toast.error(String(error)))
              }
            />
            <div className="mt-1 text-xs text-zinc-600">Automatically stops recordings after this many seconds.</div>
          </label>

          {settings.max_recording_duration > 0 && (
            <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
              <div>
                <div className="text-sm font-medium text-zinc-100">Auto-restart recording</div>
                <div className="text-xs text-zinc-500">Restart recording automatically after max duration is reached.</div>
              </div>
              <Switch
                checked={settings.auto_restart_recording}
                onCheckedChange={(checked) =>
                  updateField("auto_restart_recording", checked).catch((error: unknown) => toast.error(String(error)))
                }
              />
            </div>
          )}

          <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
            <div>
              <div className="text-sm font-medium text-zinc-100">Legacy audio flag</div>
              <div className="text-xs text-zinc-500">Fallback AAC flag when no specific audio device is selected.</div>
            </div>
            <Switch
              checked={settings.include_audio}
              onCheckedChange={(checked) =>
                updateField("include_audio", checked).catch((error: unknown) => toast.error(String(error)))
              }
            />
          </div>

          {/* Audio Control Section */}
          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Volume2 className="h-4 w-4 text-emerald-400" />
              <h3 className="text-sm font-semibold text-zinc-300">Audio Control</h3>
            </div>

            {/* Webcam Audio */}
            <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
              <div className="mb-2 flex items-center justify-between gap-3">
                <div className="flex items-center gap-3">
                  <span className="text-xs font-semibold uppercase text-zinc-400">Webcam Audio</span>
                  <AudioMeter level={settings.webcam_audio_enabled ? webcamAudioLevel : 0} className="w-24" />
                </div>
                <Switch
                  checked={settings.webcam_audio_enabled}
                  onCheckedChange={(checked) =>
                    updateField("webcam_audio_enabled", checked).catch((error: unknown) => toast.error(String(error)))
                  }
                />
              </div>
              {settings.webcam_audio_enabled && (
                <div className="flex gap-2">
                  <select
                    value={settings.webcam_audio_device}
                    onChange={(event) =>
                      updateField("webcam_audio_device", event.target.value).catch((error: unknown) => toast.error(String(error)))
                    }
                    className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                  >
                    <option value="">Default microphone</option>
                    {audioDevices.map((device) => (
                      <option key={device.name} value={device.name}>
                        {device.name}
                      </option>
                    ))}
                  </select>
                  <Button
                    variant="secondary"
                    onClick={() => void refreshAudioList()}
                    className="shrink-0"
                  >
                    <RefreshCcw className="h-4 w-4" />
                  </Button>
                </div>
              )}
            </div>

            {/* Screen Audio */}
            <div className="mt-3 rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
              <div className="mb-2 flex items-center justify-between gap-3">
                <div className="flex items-center gap-3">
                  <span className="text-xs font-semibold uppercase text-zinc-400">Screen Audio</span>
                  <AudioMeter level={settings.screen_audio_enabled ? screenAudioLevel : 0} className="w-24" />
                </div>
                <Switch
                  checked={settings.screen_audio_enabled}
                  onCheckedChange={(checked) =>
                    updateField("screen_audio_enabled", checked).catch((error: unknown) => toast.error(String(error)))
                  }
                />
              </div>
              {settings.screen_audio_enabled && (
                <>
                  <label className="mb-2 block">
                    <span className="mb-1 block text-xs text-zinc-500">Audio device</span>
                    <div className="flex gap-2">
                      <select
                        value={settings.screen_audio_device}
                        onChange={(event) =>
                          updateField("screen_audio_device", event.target.value).catch((error: unknown) => toast.error(String(error)))
                        }
                        className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                      >
                        <option value="">Select an audio device</option>
                        {audioDevices.map((device) => (
                          <option key={device.name} value={device.name}>
                            {device.name}
                          </option>
                        ))}
                      </select>
                      <Button
                        variant="secondary"
                        onClick={() => void refreshAudioList()}
                        className="shrink-0"
                      >
                        <RefreshCcw className="h-4 w-4" />
                      </Button>
                    </div>
                  </label>
                </>
              )}
            </div>
          </div>

          {/* Multi-Camera Section */}
          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Camera className="h-4 w-4 text-emerald-400" />
              <h3 className="text-sm font-semibold text-zinc-300">Multi-Camera</h3>
            </div>
            <label className="mb-3 block">
              <span className="mb-1 block text-xs text-zinc-500">Mode</span>
              <select
                value={settings.multi_camera_mode}
                onChange={(event) =>
                  updateField("multi_camera_mode", event.target.value as Settings["multi_camera_mode"]).catch((error: unknown) => toast.error(String(error)))
                }
                className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
              >
                <option value="single">Single camera</option>
                <option value="multi">Multi — simultaneous recording</option>
                <option value="quick_switch">Quick-switch — cycle with hotkeys</option>
              </select>
            </label>
            {settings.multi_camera_mode !== "single" && cameraDevices.length > 0 && (
              <div className="space-y-1">
                <span className="mb-1 block text-xs text-zinc-500">Select cameras</span>
                {cameraDevices.map((device) => {
                  const isActiveCam = settings.multi_camera_mode === "quick_switch" && settings.camera_device === device;
                  return (
                  <label
                    key={device}
                    className={`flex items-center gap-2 rounded-md border px-3 py-2 ${
                      isActiveCam
                        ? "border-emerald-600/50 bg-emerald-900/20"
                        : "border-zinc-800 bg-zinc-900/40"
                    }`}
                  >
                    <input
                      type="checkbox"
                      checked={settings.multi_camera_devices.includes(device)}
                      onChange={() => void toggleMultiCameraDevice(device)}
                      className="h-4 w-4 accent-emerald-500"
                    />
                    <span className={`truncate text-xs text-zinc-300 ${isActiveCam ? "font-bold text-emerald-300" : ""}`} title={device}>{device}</span>
                    {isActiveCam && (
                      <span className="ml-auto text-[10px] font-medium text-emerald-400">● active</span>
                    )}
                  </label>
                  );
                })}
                {settings.multi_camera_mode === "quick_switch" && settings.multi_camera_devices.length > 0 && (() => {
                  const cams = settings.multi_camera_devices;
                  const curIdx = cams.indexOf(settings.camera_device);
                  const activeIdx = curIdx >= 0 ? curIdx : 0;
                  const prevIdx = activeIdx === 0 ? cams.length - 1 : activeIdx - 1;
                  const nextIdx = (activeIdx + 1) % cams.length;
                  return (
                  <div className="mt-3 space-y-2">
                    <div className="flex items-center justify-between gap-2 rounded-md border border-emerald-700/40 bg-emerald-900/20 px-3 py-2">
                      <div className="flex min-w-0 items-center gap-1.5 text-zinc-500" title={cams[prevIdx]}>
                        <ChevronLeft className="h-4 w-4 shrink-0 text-emerald-500" />
                        <span className="truncate text-xs">{cams[prevIdx]}</span>
                      </div>
                      <span className="shrink-0 text-xs font-bold text-emerald-300" title={cams[activeIdx]}>
                        {cams[activeIdx]}
                      </span>
                      <div className="flex min-w-0 items-center gap-1.5 text-zinc-500" title={cams[nextIdx]}>
                        <span className="truncate text-xs">{cams[nextIdx]}</span>
                        <ChevronRight className="h-4 w-4 shrink-0 text-emerald-500" />
                      </div>
                    </div>
                    <p className="text-xs text-zinc-500">
                      Hotkeys: Ctrl+Shift+ArrowLeft / Ctrl+Shift+ArrowRight to cycle cameras.
                    </p>
                  </div>
                  );
                })()}
              </div>
            )}
            {settings.multi_camera_mode !== "single" && cameraDevices.length === 0 && (
              <p className="text-xs text-zinc-500">No cameras detected. Click "Refresh Devices" above.</p>
            )}
          </div>

          {/* Region Selection Section */}
          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Crosshair className="h-4 w-4 text-emerald-400" />
              <h3 className="text-sm font-semibold text-zinc-300">Screenshot Region</h3>
            </div>
            <label className="block">
              <span className="mb-1 block text-xs text-zinc-500">Capture mode</span>
              <select
                value={settings.screenshot_region_mode}
                onChange={(event) =>
                  updateField("screenshot_region_mode", event.target.value as Settings["screenshot_region_mode"]).catch((error: unknown) => toast.error(String(error)))
                }
                className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
              >
                <option value="full">Full desktop (all monitors)</option>
                <option value="primary">Primary monitor only</option>
                <option value="custom">Custom region</option>
              </select>
            </label>
            {settings.screenshot_region_mode === "custom" && (
              <div className="mt-3 space-y-3">
                {settings.custom_region.width > 0 && settings.custom_region.height > 0 && (
                  <div className="rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-xs text-zinc-400">
                    Current: {settings.custom_region.width}×{settings.custom_region.height} at ({settings.custom_region.x}, {settings.custom_region.y})
                  </div>
                )}
                <Button
                  className="w-full"
                  onClick={() => void api.startRegionSelector().catch((error: unknown) => toast.error(String(error)))}
                >
                  <Crosshair className="h-4 w-4" />
                  Select Region with Mouse
                </Button>
              </div>
            )}
          </div>

          {/* Evidence Hardening Section */}
          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Shield className="h-4 w-4 text-emerald-400" />
              <h3 className="text-sm font-semibold text-zinc-300">Evidence Hardening</h3>
            </div>
            <div className="space-y-3">
              <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                <div>
                  <div className="text-sm font-medium text-zinc-100">Watermark</div>
                  <div className="text-xs text-zinc-500">Embed timestamp + defEYE signature on captures.</div>
                </div>
                <Switch
                  checked={settings.watermark_enabled}
                  onCheckedChange={(checked) =>
                    updateField("watermark_enabled", checked).catch((error: unknown) => toast.error(String(error)))
                  }
                />
              </div>

              {/* Image Watermark */}
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-sm font-medium text-zinc-100">Image Watermark</div>
                    <div className="text-xs text-zinc-500">Overlay a custom image on captures.</div>
                  </div>
                  <Switch
                    checked={settings.watermark_image_enabled}
                    onCheckedChange={(checked) =>
                      updateField("watermark_image_enabled", checked).catch((error: unknown) => toast.error(String(error)))
                    }
                  />
                </div>
                {settings.watermark_image_enabled && (
                  <div className="mt-3 space-y-3">
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        value={settings.watermark_image_path}
                        onChange={(e) =>
                          updateField("watermark_image_path", e.target.value).catch((error: unknown) => toast.error(String(error)))
                        }
                        placeholder="C:/path/to/watermark.png"
                        className="h-8 flex-1 rounded-md border border-zinc-800 bg-zinc-950 px-2 text-xs text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
                      />
                      <Button
                        onClick={() =>
                          openDialog({ filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "bmp", "webp"] }], multiple: false })
                            .then((selected) => {
                              if (typeof selected === "string") {
                                return updateField("watermark_image_path", selected);
                              }
                            })
                            .catch((error: unknown) => toast.error(String(error)))
                        }
                      >
                        <FileImage className="h-3.5 w-3.5" />
                        Browse
                      </Button>
                    </div>
                    <div className="grid grid-cols-2 gap-3">
                      <label className="block">
                        <span className="mb-1 block text-xs text-zinc-500">Opacity ({Math.round(settings.watermark_opacity * 100)}%)</span>
                        <input
                          type="range"
                          min={0}
                          max={1}
                          step={0.05}
                          value={settings.watermark_opacity}
                          onChange={(e) =>
                            updateField("watermark_opacity", parseFloat(e.target.value)).catch((error: unknown) => toast.error(String(error)))
                          }
                          className="h-2 w-full accent-emerald-500"
                        />
                      </label>
                      <label className="block">
                        <span className="mb-1 block text-xs text-zinc-500">Scale ({Math.round(settings.watermark_scale * 100)}%)</span>
                        <input
                          type="range"
                          min={0.01}
                          max={1}
                          step={0.01}
                          value={settings.watermark_scale}
                          onChange={(e) =>
                            updateField("watermark_scale", parseFloat(e.target.value)).catch((error: unknown) => toast.error(String(error)))
                          }
                          className="h-2 w-full accent-emerald-500"
                        />
                      </label>
                    </div>
                    <label className="block">
                      <span className="mb-1 block text-xs text-zinc-500">Position</span>
                      <select
                        value={settings.watermark_position}
                        onChange={(e) =>
                          updateField("watermark_position", e.target.value as Settings["watermark_position"]).catch((error: unknown) => toast.error(String(error)))
                        }
                        className="h-8 w-full rounded-md border border-zinc-800 bg-zinc-950 px-2 text-xs text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
                      >
                        <option value="top_left">Top Left</option>
                        <option value="top_right">Top Right</option>
                        <option value="bottom_left">Bottom Left</option>
                        <option value="bottom_right">Bottom Right</option>
                        <option value="center">Center</option>
                        <option value="custom">Custom</option>
                      </select>
                    </label>
                    {settings.watermark_position === "custom" && (
                      <div className="grid grid-cols-2 gap-3">
                        <label className="block">
                          <span className="mb-1 block text-xs text-zinc-500">X offset (px)</span>
                          <input
                            type="number"
                            min={0}
                            value={settings.watermark_x}
                            onChange={(e) =>
                              updateField("watermark_x", parseInt(e.target.value, 10) || 0).catch((error: unknown) => toast.error(String(error)))
                            }
                            className="h-8 w-full rounded-md border border-zinc-800 bg-zinc-950 px-2 text-xs text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
                          />
                        </label>
                        <label className="block">
                          <span className="mb-1 block text-xs text-zinc-500">Y offset (px)</span>
                          <input
                            type="number"
                            min={0}
                            value={settings.watermark_y}
                            onChange={(e) =>
                              updateField("watermark_y", parseInt(e.target.value, 10) || 0).catch((error: unknown) => toast.error(String(error)))
                            }
                            className="h-8 w-full rounded-md border border-zinc-800 bg-zinc-950 px-2 text-xs text-zinc-100 outline-none focus:border-emerald-500"
                          />
                        </label>
                      </div>
                    )}
                  </div>
                )}
              </div>
              <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                <div>
                  <div className="text-sm font-medium text-zinc-100">Embed metadata</div>
                  <div className="text-xs text-zinc-500">Add MP4 metadata tags and sidecar JSON files.</div>
                </div>
                <Switch
                  checked={settings.embed_metadata}
                  onCheckedChange={(checked) =>
                    updateField("embed_metadata", checked).catch((error: unknown) => toast.error(String(error)))
                  }
                />
              </div>
              <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                <div>
                  <div className="text-sm font-medium text-zinc-100">Integrity check (SHA256)</div>
                  <div className="text-xs text-zinc-500">Compute and store SHA256 hash in sidecar JSON for tamper detection.</div>
                </div>
                <Switch
                  checked={settings.integrity_check}
                  onCheckedChange={(checked) =>
                    updateField("integrity_check", checked).catch((error: unknown) => toast.error(String(error)))
                  }
                />
              </div>
            </div>
          </div>

          <div className="border-t border-zinc-800/70 pt-5">
            <h3 className="mb-4 text-sm font-semibold text-zinc-300">Screen Recording</h3>

            <label className="block">
              <div className="mb-2 flex items-center justify-between text-sm">
                <span className="text-zinc-400">FPS</span>
                <span className="font-mono text-emerald-300">{settings.screen_fps}</span>
              </div>
              <input
                type="range"
                min={5}
                max={60}
                step={1}
                value={settings.screen_fps}
                onChange={(event) =>
                  updateField("screen_fps", Number(event.target.value)).catch((error: unknown) => toast.error(String(error)))
                }
                className="w-full accent-emerald-500"
              />
              <div className="mt-1 flex justify-between text-xs text-zinc-600">
                <span>5</span>
                <span>60</span>
              </div>
            </label>

            <label className="mt-3 block">
              <div className="mb-2 flex items-center justify-between text-sm">
                <span className="text-zinc-400">CRF quality</span>
                <span className="font-mono text-emerald-300">{settings.screen_crf}</span>
              </div>
              <input
                type="range"
                min={18}
                max={32}
                step={1}
                value={settings.screen_crf}
                onChange={(event) =>
                  updateField("screen_crf", Number(event.target.value)).catch((error: unknown) => toast.error(String(error)))
                }
                className="w-full accent-emerald-500"
              />
              <div className="mt-1 flex justify-between text-xs text-zinc-600">
                <span>18</span>
                <span>32</span>
              </div>
            </label>

            {/* Screen capture target */}
            <div className="mt-4 rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
              <span className="mb-2 block text-xs font-semibold uppercase tracking-wider text-zinc-400">Capture Target</span>
              <label className="mb-2 block">
                <div className="flex gap-2">
                  <select
                    value={settings.screen_capture_mode === "specific_monitor" ? settings.screen_monitor_id : ""}
                    onChange={(event) => {
                      const monitorId = event.target.value;
                      if (screenActive) {
                        void api.switchScreenMonitor(monitorId).catch((error: unknown) => toast.error(String(error)));
                      } else {
                        if (monitorId) {
                          void updateField("screen_capture_mode", "specific_monitor" as const).then(() =>
                            updateField("screen_monitor_id", monitorId).catch((error: unknown) => toast.error(String(error)))
                          );
                        } else {
                          void updateField("screen_capture_mode", "all_monitors" as const).then(() =>
                            updateField("screen_monitor_id", "").catch((error: unknown) => toast.error(String(error)))
                        );
                        }
                      }
                    }}
                    className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                  >
                    <option value="">All monitors (full desktop)</option>
                    {monitors.map((m) => (
                      <option key={m.id} value={m.id}>
                        {monitorLabel(m)}
                      </option>
                    ))}
                  </select>
                  <Button
                    variant="secondary"
                    onClick={() => void refreshMonitorList()}
                    className="shrink-0"
                  >
                    <RefreshCcw className="h-4 w-4" />
                  </Button>
                </div>
              </label>
              {screenActive && (
                <p className="text-xs text-emerald-400">
                  Recording active — selecting a different target will restart the recording on the new monitor.
                </p>
              )}
            </div>

            <p className="mt-2 text-xs text-zinc-500">Uses gdigrab. Lower FPS reduces CPU and file size.</p>
          </div>

          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Radar className={`h-4 w-4 ${settings.motion_mode_enabled ? "text-amber-400" : "text-zinc-500"}`} />
              <h3 className="text-sm font-semibold text-zinc-300">Sentinel Motion Mode</h3>
              {motionStatus.motion_active && (
                <span className="ml-auto inline-flex items-center gap-1 rounded-full border border-amber-500/50 bg-amber-500/10 px-2 py-0.5 text-xs font-semibold text-amber-300">
                  <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-400" />
                  SCANNING
                </span>
              )}
            </div>

            <div className={`flex items-center justify-between rounded-lg border px-4 py-3 transition-all duration-200 ${settings.motion_mode_enabled ? "border-amber-500/30 bg-amber-950/20" : "border-zinc-800/70 bg-zinc-900/60"}`}>
              <div>
                <div className="text-sm font-medium text-zinc-100">Enable Motion Detection</div>
                <div className="text-xs text-zinc-500">Background scene-change monitoring on webcam. Hotkey: Ctrl+Shift+ArrowUp</div>
              </div>
              <Switch
                sfx={false}
                checked={settings.motion_mode_enabled}
                onCheckedChange={(checked) =>
                  updateField("motion_mode_enabled", checked).catch((error: unknown) => toast.error(String(error)))
                }
              />
            </div>

            {settings.motion_mode_enabled && (
              <div className="mt-4 space-y-4">
                {motionStatus.motion_active && (
                  <div className="flex items-center gap-2 rounded-lg border border-amber-500/20 bg-amber-950/10 px-4 py-2 text-xs text-amber-300">
                    <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-400" />
                    Scanning{motionStatus.last_detection ? ` • Last detection: ${formatTimestamp(motionStatus.last_detection)}` : ""}
                  </div>
                )}
                {!motionStatus.motion_active && motionStatus.last_detection && (
                  <p className="text-xs text-zinc-500">
                    Last detection: {formatTimestamp(motionStatus.last_detection)}
                  </p>
                )}
                <label className="block">
                  <div className="mb-2 flex items-center justify-between text-sm">
                    <span className="text-zinc-400">Sensitivity</span>
                    <span className="font-mono text-emerald-300">{settings.motion_sensitivity}</span>
                  </div>
                  <input
                    type="range"
                    min={1}
                    max={100}
                    step={1}
                    value={settings.motion_sensitivity}
                    onChange={(event) =>
                      updateField("motion_sensitivity", Number(event.target.value)).catch((error: unknown) => toast.error(String(error)))
                    }
                    className="w-full accent-amber-500"
                  />
                  <div className="mt-1 flex justify-between text-xs text-zinc-600">
                    <span>1 (low)</span>
                    <span>100 (high)</span>
                  </div>
                </label>

                <label className="block">
                  <span className="mb-2 block text-sm text-zinc-400">Cooldown (seconds)</span>
                  <Input
                    type="number"
                    min={1}
                    value={settings.motion_cooldown_seconds}
                    onChange={(event) =>
                      updateField("motion_cooldown_seconds", Math.max(1, Number(event.target.value))).catch((error: unknown) => toast.error(String(error)))
                    }
                  />
                </label>

                <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                  <div>
                    <div className="text-sm font-medium text-zinc-100">Auto-record on motion</div>
                    <div className="text-xs text-zinc-500">Start webcam recording when motion is detected.</div>
                  </div>
                  <Switch
                    checked={settings.auto_record_on_motion}
                    onCheckedChange={(checked) =>
                      updateField("auto_record_on_motion", checked).catch((error: unknown) => toast.error(String(error)))
                    }
                  />
                </div>

                {settings.auto_record_on_motion && (
                  <div className="grid grid-cols-2 gap-3">
                    <label className="block">
                      <span className="mb-2 block text-sm text-zinc-400">Min record (seconds)</span>
                      <Input
                        type="number"
                        min={1}
                        value={settings.motion_min_record_seconds}
                        onChange={(event) =>
                          updateField("motion_min_record_seconds", Math.max(1, Number(event.target.value))).catch((error: unknown) => toast.error(String(error)))
                        }
                      />
                      <div className="mt-1 text-xs text-zinc-600">Minimum recording time once motion triggers.</div>
                    </label>
                    <label className="block">
                      <span className="mb-2 block text-sm text-zinc-400">Post-record (seconds)</span>
                      <Input
                        type="number"
                        min={0}
                        value={settings.motion_post_record_seconds}
                        onChange={(event) =>
                          updateField("motion_post_record_seconds", Math.max(0, Number(event.target.value))).catch((error: unknown) => toast.error(String(error)))
                        }
                      />
                      <div className="mt-1 text-xs text-zinc-600">Keep recording this long after motion stops.</div>
                    </label>
                  </div>
                )}

                <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
                  <div>
                    <div className="text-sm font-medium text-zinc-100">Also trigger screen recording</div>
                    <div className="text-xs text-zinc-500">Start screen recording alongside webcam on motion.</div>
                  </div>
                  <Switch
                    checked={settings.motion_triggers_screen}
                    onCheckedChange={(checked) =>
                      updateField("motion_triggers_screen", checked).catch((error: unknown) => toast.error(String(error)))
                    }
                  />
                </div>

                {motionStatus.last_detection && (
                  <p className="text-xs text-zinc-500">
                    Last detection: {formatTimestamp(motionStatus.last_detection)}
                  </p>
                )}
              </div>
            )}
          </div>

          <div className="border-t border-zinc-800/70 pt-5">
            <div className="mb-4 flex items-center gap-2">
              <Monitor className="h-4 w-4 text-emerald-400" />
              <h3 className="text-sm font-semibold text-zinc-300">HUD & Screen Target</h3>
            </div>
          </div>

          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Primary screen target</span>
            <div className="flex gap-2">
              <select
                value={settings.primary_monitor_id}
                onChange={(event) =>
                  updateField("primary_monitor_id", event.target.value).catch((error: unknown) => toast.error(String(error)))
                }
                className="h-9 min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
              >
                <option value="">System primary monitor</option>
                {monitors.map((monitor) => (
                  <option key={monitor.id} value={monitor.id}>
                    {monitorLabel(monitor)}
                  </option>
                ))}
              </select>
              <Button onClick={() => void refreshMonitorList()}>
                <RefreshCcw className="h-4 w-4" />
                Refresh
              </Button>
            </div>
          </label>

          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">HUD corner</span>
            <select
              value={settings.hud_corner}
              onChange={(event) =>
                updateField("hud_corner", event.target.value as HudCorner).catch((error: unknown) => toast.error(String(error)))
              }
              className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
            >
              <option value="top_right">Top right</option>
              <option value="top_left">Top left</option>
              <option value="bottom_right">Bottom right</option>
              <option value="bottom_left">Bottom left</option>
              <option value="hidden">Hidden</option>
            </select>
          </label>

          <label className="block">
            <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
              <div>
                <div className="text-sm font-medium text-zinc-100">Minimal HUD</div>
                <div className="text-xs text-zinc-500">Compact dot-only indicator — no text label, reduced frame size.</div>
              </div>
              <Switch
                checked={settings.hud_minimal}
                onCheckedChange={(checked) =>
                  updateField("hud_minimal", checked).catch((error: unknown) => toast.error(String(error)))
                }
              />
            </div>
          </label>

          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Output directory</span>
            <div className="flex gap-2">
              <Input value={settings.output_dir} readOnly />
              <Button className="text-xs px-2 py-1" onClick={() => void changeFolder()}>
                <FolderOpen className="h-3.5 w-3.5" />
                Change Folder
              </Button>
            </div>
          </label>
        </div>
      </Card>

      <Card className="p-5">
        <div className="mb-5 flex items-center gap-2">
          <Play className="h-4 w-4 text-emerald-400" />
          <h2 className="text-base font-semibold text-zinc-50">Actions</h2>
        </div>
        <div className="grid gap-2.5">
          <Button
            variant="primary"
            className={`h-10 ${webcamActive ? "sentinel-recording-active" : ""}`}
            disabled={webcamActive || multiActive || finalizing || busyAction === "start"}
            sfx={false}
            onClick={() => void runAction("start", api.startRecording, "Recording started")}
          >
            {busyAction === "start" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Video className="h-4 w-4" />}
            Start Webcam
          </Button>
          <Button
            variant="danger"
            disabled={!webcamActive || finalizing || busyAction === "stop"}
            sfx={false}
            onClick={() => void runAction("stop", api.stopRecording, "Webcam recording stopped")}
          >
            {busyAction === "stop" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
            Stop Recording
          </Button>
        </div>
        <div className="mt-4 border-t border-zinc-800/70 pt-4">
          <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500">Quick Capture</h3>
          <div className="grid gap-2.5">
            <Button
              disabled={busyAction === "current"}
              sfx="capture-primary"
              onClick={() => { toast.info("Capturing primary screen…"); void runAction("current", api.captureCurrent, "Primary screen captured"); }}
            >
              <Crosshair className="h-4 w-4" />
              Capture Primary
            </Button>
            <Button
              disabled={busyAction === "merged"}
              sfx="capture-all"
              onClick={() => { toast.info("Capturing all screens merged…"); void runAction("merged", api.captureAllMerged, "All screens captured"); }}
            >
              <Layers className="h-4 w-4" />
              Capture All Screens
            </Button>
          </div>
        </div>

        <div className="mt-4 border-t border-zinc-800/70 pt-4">
          <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500">Screen Recording</h3>
          <div className="grid gap-2.5">
            <Button
              variant="primary"
              className={`h-10 ${screenActive ? "sentinel-recording-active" : ""}`}
              disabled={screenActive || finalizing || busyAction === "startScreen"}
              sfx={false}
              onClick={() => void runAction("startScreen", api.startScreenRecording, "Screen recording started")}
            >
              {busyAction === "startScreen" ? <Loader2 className="h-4 w-4 animate-spin" /> : <MonitorDown className="h-4 w-4" />}
              Start Screen Record
            </Button>
            <Button
              variant="danger"
              disabled={!screenActive || finalizing || busyAction === "stopScreen"}
              sfx={false}
              onClick={() => void runAction("stopScreen", api.stopScreenRecording, "Screen recording stopped")}
            >
              {busyAction === "stopScreen" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-4 w-4" />}
              Stop Screen Record
            </Button>
          </div>
        </div>
      </Card>

      <Card className={`flex flex-col justify-center p-5 transition-all duration-200 ${timelapseStatus.active ? "border-emerald-500/30" : ""}`}>
        <div className="mb-5 flex items-center gap-2">
          <Clock className={`h-4 w-4 ${timelapseStatus.active ? "text-emerald-400" : "text-violet-400"}`} />
          <h2 className="text-base font-semibold text-zinc-50">Time-Lapse Capture</h2>
          {timelapseStatus.active && (
            <span className="ml-auto inline-flex items-center gap-1 rounded-full border border-emerald-500/50 bg-emerald-500/10 px-2 py-0.5 text-xs font-semibold text-emerald-300">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-400" />
              ACTIVE
            </span>
          )}
        </div>
        <div className="grid gap-4">
          <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
            <div className="mb-3 text-xs text-zinc-500">Capture frames at regular intervals. Hotkey: Ctrl+Alt+ArrowRight</div>
            <div className="grid grid-cols-2 gap-3">
              <label className="block">
                <span className="mb-1 block text-xs text-zinc-400">Interval (seconds, min 5)</span>
                <Input
                  type="number"
                  min={0}
                  value={settings.timelapse_interval_seconds}
                  onChange={(e) => updateField("timelapse_interval_seconds", Number(e.target.value)).catch((error: unknown) => toast.error(String(error)))}
                  className="w-full"
                />
              </label>
              <label className="block">
                <span className="mb-1 block text-xs text-zinc-400">Target</span>
                <select
                  value={settings.timelapse_target}
                  onChange={(e) => updateField("timelapse_target", e.target.value as Settings["timelapse_target"]).catch((error: unknown) => toast.error(String(error)))}
                  className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-violet-500 focus:ring-1 focus:ring-violet-500"
                >
                  <option value="screen">Screen</option>
                  <option value="webcam">Webcam</option>
                  <option value="both">Both (Screen + Webcam)</option>
                </select>
              </label>
            </div>
          </div>
          {timelapseStatus.active ? (
            <Button variant="danger" className="h-10" onClick={() => void api.stopTimelapse().catch((error: unknown) => toast.error(String(error)))}>
              <Square className="h-4 w-4" />
              Stop Time-Lapse
            </Button>
          ) : (
            <Button
              variant="primary"
              className="h-10"
              disabled={settings.timelapse_interval_seconds < 5}
              sfx={false}
              onClick={() => void api.startTimelapse().catch((error: unknown) => toast.error(String(error)))}
            >
              <Clock className="h-4 w-4" />
              Start Time-Lapse
            </Button>
          )}
        </div>
      </Card>

      <Card className="p-5 opacity-95">
        <div className="mb-5 flex items-center gap-2">
          <Shield className="h-4 w-4 text-emerald-400/80" />
          <h2 className="text-base font-semibold text-zinc-50">Sentinel Systems</h2>
        </div>
        <div className="grid gap-4">
          <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
            <div>
              <div className="text-sm font-medium text-zinc-100">Watchdog Auto-Recovery</div>
              <div className="text-xs text-zinc-500">Detect ffmpeg crashes and automatically restart recordings.</div>
              {settings.watchdog_enabled && (
                <div className="mt-1.5 flex items-center gap-1 text-xs text-emerald-400/80">
                  <span className="h-1 w-1 rounded-full bg-emerald-400" />
                  Active — monitoring ffmpeg processes
                </div>
              )}
            </div>
            <Switch
              checked={settings.watchdog_enabled}
              onCheckedChange={(checked) => updateField("watchdog_enabled", checked).catch((error: unknown) => toast.error(String(error)))}
            />
          </div>

          <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm font-medium text-zinc-100">Disk Sentinel</div>
                <div className="text-xs text-zinc-500">Stop recordings when free disk space drops below threshold.</div>
              </div>
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  min={0}
                  value={settings.disk_threshold_mb}
                  onChange={(e) => updateField("disk_threshold_mb", Number(e.target.value)).catch((error: unknown) => toast.error(String(error)))}
                  className="w-24 text-xs"
                />
                <span className="text-xs text-zinc-500">MB</span>
              </div>
            </div>
            {diskInfo && (
              <div className="mt-3 flex items-center gap-2 text-xs">
                <HardDrive className={`h-3 w-3 ${diskInfo.warning ? "text-red-400" : "text-emerald-400"}`} />
                <span className={diskInfo.warning ? "text-red-300" : "text-zinc-400"}>
                  {diskInfo.free_mb >= 1024
                    ? `${(diskInfo.free_mb / 1024).toFixed(1)} GB free`
                    : `${diskInfo.free_mb} MB free`}
                  {" / "}
                  {diskInfo.total_bytes >= 1024 * 1024 * 1024
                    ? `${(diskInfo.total_bytes / (1024 * 1024 * 1024)).toFixed(1)} GB total`
                    : `${(diskInfo.total_bytes / (1024 * 1024)).toFixed(0)} MB total`}
                </span>
                {diskInfo.warning && (
                  <span className="font-semibold text-red-400">— WARNING: Below threshold!</span>
                )}
              </div>
            )}
          </div>
        </div>
      </Card>
    </div>
  );
}

function monitorLabel(monitor: MonitorInfo) {
  const primary = monitor.is_primary ? " primary" : "";
  return `${monitor.friendly_name || monitor.name} (${monitor.width}x${monitor.height} @ ${monitor.x},${monitor.y}${primary})`;
}

interface CapturesTabProps {
  captures: CaptureInfo[];
  outputDir: string;
  refreshCaptures: () => Promise<void>;
  setSelectedCapture: (path: string) => void;
  timelapseStatus: TimelapseStatusPayload;
  settings: Settings;
  saveSettings: (settings: Settings) => Promise<void>;
}

function CapturesTab({ captures, outputDir, refreshCaptures, setSelectedCapture, timelapseStatus, settings, saveSettings }: CapturesTabProps) {
  const [verifying, setVerifying] = useState<string | null>(null);
  const [integrityResult, setIntegrityResult] = useState<IntegrityResult | null>(null);
  const [previewCapture, setPreviewCapture] = useState<CaptureInfo | null>(null);
  const [noteCapture, setNoteCapture] = useState<CaptureInfo | null>(null);
  const [noteText, setNoteText] = useState("");
  const [extracting, setExtracting] = useState(false);
  const [timelapseExpanded, setTimelapseExpanded] = useState(true);
  const [deleteSession, setDeleteSession] = useState<string | null>(null);
  const [sessionGifs, setSessionGifs] = useState<Record<string, string>>({});

  const timelapseCaptures = captures.filter((c) => c.kind === "timelapse");
  const otherCaptures = captures.filter((c) => c.kind !== "timelapse");

  // Track which timelapse paths we've seen before to animate new ones
  const prevTimelapsePaths = useRef<Set<string>>(new Set());
  const newTimelapsePaths = useMemo(() => {
    const prev = prevTimelapsePaths.current;
    const newSet = new Set<string>();
    for (const c of timelapseCaptures) {
      if (!prev.has(c.path)) newSet.add(c.path);
    }
    prevTimelapsePaths.current = new Set(timelapseCaptures.map((c) => c.path));
    return newSet;
  }, [timelapseCaptures]);

  // Group timelapse captures by session folder
  const timelapseSessions = useMemo(() => {
    const groups = new Map<string, typeof timelapseCaptures>();
    for (const c of timelapseCaptures) {
      const key = c.session ?? "ungrouped";
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)!.push(c);
    }
    return Array.from(groups.entries()).sort(([a], [b]) => b.localeCompare(a));
  }, [timelapseCaptures]);

  // Detect existing GIFs in timelapse session directories
  useEffect(() => {
    for (const [sessionName] of timelapseSessions) {
      if (sessionGifs[sessionName] !== undefined) continue;
      const gifPath = `${outputDir}/timelapse/${sessionName}/timelapse.gif`;
      const img = new Image();
      img.onload = () => {
        setSessionGifs((prev) => ({ ...prev, [sessionName]: gifPath }));
      };
      img.src = convertFileSrc(gifPath);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [timelapseSessions, outputDir]);

  const updateField = async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    await saveSettings({ ...settings, [key]: value });
  };

  const deleteCapture = async (path: string) => {
    if (!window.confirm("Delete this capture?")) {
      return;
    }
    playSfx("trash");
    try {
      await api.deleteCapture(path);
      toast.success("Capture deleted");
      await refreshCaptures();
    } catch (error) {
      toast.error(String(error));
    }
  };

  const verifyCapture = async (path: string) => {
    try {
      setVerifying(path);
      const result = await api.verifyIntegrity(path);
      setIntegrityResult(result);
      if (result.verified) {
        toast.success("Integrity verified: SHA256 matches");
      } else if (result.stored_hash === null) {
        toast.info("No integrity sidecar found");
      } else {
        toast.error("Integrity check FAILED: hash mismatch");
      }
    } catch (error) {
      toast.error(String(error));
    } finally {
      setVerifying(null);
    }
  };

  const openNote = async (capture: CaptureInfo) => {
    setNoteCapture(capture);
    try {
      const result = await api.getCaptureNote(capture.path);
      setNoteText(result.note ?? "");
    } catch {
      setNoteText("");
    }
  };

  const saveNote = async () => {
    if (!noteCapture) return;
    try {
      await api.setCaptureNote(noteCapture.path, noteText);
      toast.success(noteText.trim() ? "Note saved" : "Note cleared");
      setNoteCapture(null);
      await refreshCaptures();
    } catch (error) {
      toast.error(String(error));
    }
  };

  const snapFrame = async (capture: CaptureInfo, currentTime: number) => {
    try {
      setExtracting(true);
      const result = await api.extractSnapshot(capture.path, currentTime);
      toast.success(`Frame saved: ${result}`);
      await refreshCaptures();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setExtracting(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-5">
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-3">
          <h2 className="text-base font-semibold text-zinc-50">Recent Captures</h2>
          {captures.length > 0 && (
            <span className="text-xs text-zinc-500">
              {captures.length} file{captures.length !== 1 ? "s" : ""} · {formatBytes(captures.reduce((sum, c) => sum + c.size, 0))}
            </span>
          )}
        </div>
        <div className="ml-auto mr-4 flex gap-2">
          <Button onClick={() => void api.openPath(outputDir).catch((error: unknown) => toast.error(String(error)))}>
            <FolderOpen className="h-4 w-4" />
            Open Folder
          </Button>
          <Button onClick={() => void refreshCaptures().catch((error: unknown) => toast.error(String(error)))}>
            <RefreshCcw className="h-4 w-4" />
            Refresh
          </Button>
        </div>
      </div>

      {integrityResult && (
        <Card className="p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="flex items-center gap-2">
              {integrityResult.verified ? (
                <ShieldCheck className="h-5 w-5 text-emerald-400" />
              ) : (
                <Shield className="h-5 w-5 text-amber-400" />
              )}
              <div>
                <div className="text-sm font-medium text-zinc-100">{integrityResult.message}</div>
                {integrityResult.stored_hash && (
                  <div className="mt-1 text-xs text-zinc-500">
                    Stored: <span className="font-mono">{integrityResult.stored_hash.substring(0, 16)}...</span>
                  </div>
                )}
                {integrityResult.actual_hash && (
                  <div className="text-xs text-zinc-500">
                    Actual: <span className="font-mono">{integrityResult.actual_hash.substring(0, 16)}...</span>
                  </div>
                )}
              </div>
            </div>
            <Button variant="ghost" onClick={() => setIntegrityResult(null)}>
              <XCircle className="h-4 w-4" />
            </Button>
          </div>
        </Card>
      )}

      {timelapseCaptures.length > 0 && (
        <Card className="overflow-hidden">
          <button
            className="flex w-full items-center justify-between px-4 py-3 text-left transition hover:bg-zinc-900/50"
            onClick={() => { playSfx("button-click-else"); setTimelapseExpanded((v) => !v) }}
          >
            <div className="flex items-center gap-2">
              <Clock className="h-4 w-4 text-violet-400" />
              <span className="text-sm font-semibold text-zinc-100">Time-Lapse Sequence</span>
              <span className="rounded-full bg-violet-900/40 px-2 py-0.5 text-xs text-violet-300">
                {timelapseCaptures.length} frame{timelapseCaptures.length !== 1 ? "s" : ""}
              </span>
              {timelapseStatus.active && (
                <span className="flex items-center gap-1 text-xs text-emerald-300">
                  <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-400" />
                  Recording · every {timelapseStatus.interval_seconds}s
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              {timelapseExpanded ? (
                <ChevronDown className="h-4 w-4 text-zinc-500" />
              ) : (
                <ChevronRight className="h-4 w-4 text-zinc-500" />
              )}
            </div>
          </button>
          {timelapseExpanded && (
            <div className="border-t border-zinc-800/70 p-4">
              <div className="flex flex-col gap-3 max-h-[320px] overflow-y-auto timelapse-scroll-y">
                {timelapseSessions.map(([sessionName, sessionCaptures]) => (
                  <div key={sessionName} className="rounded-lg border border-zinc-800/60 bg-zinc-900/30 p-3">
                    <div className="mb-2 flex items-center gap-2 px-1">
                      <span className="text-xs font-semibold text-violet-300">
                        {sessionName === "ungrouped" ? "Un grouped" : sessionName.replace("session_", "").replace(/_/g, " ")}
                      </span>
                      <span className="text-[10px] text-zinc-600">
                        {sessionCaptures.length} capture{sessionCaptures.length !== 1 ? "s" : ""}
                      </span>
                      <button
                        onClick={() => { playSfx("button-click-else"); setDeleteSession(sessionName) }}
                        className="ml-auto flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900 px-2 py-0.5 text-[10px] text-zinc-400 transition-colors hover:border-red-600/50 hover:text-red-400"
                        title="Delete entire session"
                      >
                        <Trash2 className="h-3 w-3" />
                        Delete Session
                      </button>
                      <button
                        onClick={() => { playSfx("button-click-else"); void api.openPath(`${outputDir}/timelapse/${sessionName}`).catch((error: unknown) => toast.error(String(error))) }}
                        className="flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900 px-2 py-0.5 text-[10px] text-zinc-400 transition-colors hover:border-violet-600/50 hover:text-violet-300"
                        title="Open session folder"
                      >
                        <FolderOpen className="h-3 w-3" />
                      </button>
                      <button
                        onClick={() => { playSfx("button-click-else"); void (async () => {
                          try {
                            toast.info("Creating GIF...");
 const result = await api.createTimelapseGif(sessionName);
                            toast.success(`GIF created: ${result.split(/[\\/]/).pop()}`);
                            setSessionGifs((prev) => ({ ...prev, [sessionName]: result }));
                            await refreshCaptures();
                          } catch (error) {
                            toast.error(String(error));
                          }
                        })() }}
                        className="flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900 px-2 py-0.5 text-[10px] text-zinc-400 transition-colors hover:border-violet-600/50 hover:text-violet-300"
                        title="Create animated GIF from session"
                      >
                        <Film className="h-3 w-3" />
                        Create GIF
                      </button>
                    </div>
                    <div className="flex gap-2 overflow-x-auto pb-2 timelapse-scroll">
                      {sessionGifs[sessionName] && (
                        <div
                          className="group relative shrink-0 overflow-hidden rounded-lg border-2 border-violet-500/60 bg-zinc-900 timelapse-gif-glow"
                          style={{ width: "160px" }}
                          title="Animated GIF — LMB: Open"
                          onClick={() => void api.openPath(sessionGifs[sessionName]).catch((error: unknown) => toast.error(String(error)))}
                        >
                          <div className="aspect-video cursor-pointer">
                            <img
                              src={convertFileSrc(sessionGifs[sessionName])}
                              alt="timelapse.gif"
                              className="h-full w-full object-cover"
                            />
                          </div>
                          <div className="absolute bottom-1 left-1 flex items-center gap-1 rounded bg-black/60 px-1.5 py-0.5 text-[9px] font-medium text-violet-300">
                            <Film className="h-2.5 w-2.5" />
                            GIF
                          </div>
                        </div>
                      )}
                      {sessionCaptures.map((capture) => (
                        <div
                          key={capture.path}
                          className={`group relative shrink-0 overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900 transition hover:border-violet-600 ${newTimelapsePaths.has(capture.path) ? "animate-timelapse-slide-in" : ""}`}
                          style={{ width: "160px" }}
                          title="LMB: Preview · RMB: Delete"
                          onContextMenu={(e) => { e.preventDefault(); void deleteCapture(capture.path); }}
                        >
                          <div
                            className="aspect-video cursor-pointer"
                            onClick={() => setPreviewCapture(capture)}
                          >
                            {capture.thumbnail ? (
                              <img
                                src={convertFileSrc(capture.thumbnail)}
                                alt={capture.filename}
                                className="h-full w-full object-cover"
                              />
                            ) : (
                              <div className="flex h-full items-center justify-center">
                                <FileImage className="h-6 w-6 text-zinc-700" />
                              </div>
                            )}
                          </div>
                          <div className="absolute right-1 top-1 flex gap-0.5 opacity-0 transition group-hover:opacity-100">
                            <Button
                              variant="ghost"
                              className="h-5 w-5 p-0"
                              onClick={() => void openNote(capture)}
                              title="Add/edit note"
                            >
                              <StickyNote className="h-3 w-3" />
                            </Button>
                            <Button
                              variant="ghost"
                              className="h-5 w-5 p-0"
                              onClick={() => void deleteCapture(capture.path)}
                              title="Delete"
                            >
                              <Trash2 className="h-3 w-3" />
                            </Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </Card>
      )}

      {deleteSession && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setDeleteSession(null)}>
          <div className="mx-4 w-full max-w-sm rounded-lg border border-red-600/40 bg-zinc-900 p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-red-900/50">
                <Trash2 className="h-5 w-5 text-red-400" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-zinc-50">Delete Session?</h3>
                <p className="mt-1 text-sm text-zinc-400">
                  This will permanently delete all {timelapseSessions.find(([s]) => s === deleteSession)?.[1].length ?? 0} capture(s) in this time-lapse session. This cannot be undone.
                </p>
              </div>
            </div>
            <div className="mt-5 flex justify-end gap-2">
              <Button onClick={() => setDeleteSession(null)}>
                Cancel
              </Button>
              <Button variant="danger" onClick={() => void (async () => {
                if (!deleteSession) return;
                try {
                  await api.deleteTimelapseSession(deleteSession);
                  toast.success("Time-lapse session deleted");
                  await refreshCaptures();
                } catch (error) {
                  toast.error(String(error));
                } finally {
                  setDeleteSession(null);
                }
              })()}>
                <Trash2 className="h-4 w-4" />
                Delete
              </Button>
            </div>
          </div>
        </div>
      )}

      <Card className="min-h-0 flex-1 overflow-hidden">
        {otherCaptures.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-zinc-500">
            {timelapseCaptures.length > 0 ? "No other captures." : "No captures yet."}
          </div>
        ) : (
          <div className="h-full overflow-y-auto overflow-x-hidden">
            <table className="w-full text-left text-sm">
              <thead className="sticky top-0 bg-zinc-950 text-xs uppercase text-zinc-500">
                <tr className="border-b border-zinc-800">
                  <th className="w-16 px-3 py-3">Preview</th>
                  <th className="w-12 px-3 py-3">Type</th>
                  <th className="px-3 py-3">Filename</th>
                  <th className="w-36 px-3 py-3">Timestamp</th>
                  <th className="w-20 px-3 py-3">Duration</th>
                  <th className="w-20 px-3 py-3">Size</th>
                  <th className="w-44 px-3 py-3">Actions</th>
                </tr>
              </thead>
              <tbody>
                {otherCaptures.map((capture) => (
                  <tr key={capture.path} className="border-b border-zinc-900">
                    <td className="px-3 py-3">
                      {capture.thumbnail ? (
                        <img
                          src={convertFileSrc(capture.thumbnail)}
                          alt={capture.filename}
                          className="h-10 w-16 rounded border border-zinc-700 object-cover"
                        />
                      ) : (
                        <div className="flex h-10 w-16 items-center justify-center rounded border border-zinc-800 bg-zinc-900">
                          {capture.kind === "webcam" || capture.kind === "screen" ? (
                            <Video className="h-4 w-4 text-zinc-600" />
                          ) : (
                            <FileImage className="h-4 w-4 text-zinc-600" />
                          )}
                        </div>
                      )}
                    </td>
                    <td className="px-3 py-3">
                      <div className="flex items-center gap-1">
                        {capture.kind === "webcam" || capture.kind === "screen" ? (
                          <Video className="h-4 w-4 text-emerald-300" />
                        ) : (
                          <FileImage className="h-4 w-4 text-zinc-300" />
                        )}
                        {capture.has_integrity && (
                          <span title="Integrity sidecar present — SHA256 hash stored alongside this capture for tamper verification"><ShieldCheck className="h-3 w-3 text-emerald-400" /></span>
                        )}
                        {capture.has_watermark && (
                          <span title="Watermarked"><Shield className="h-3 w-3 text-blue-400" /></span>
                        )}
                        {capture.has_note && (
                          <span title="Has annotation"><StickyNote className="h-3 w-3 text-amber-400" /></span>
                        )}
                      </div>
                    </td>
                    <td className="max-w-0 px-3 py-3">
                      <div className="group relative overflow-hidden">
                        <div
                          className="whitespace-nowrap font-mono text-xs text-zinc-200 transition-all duration-300 group-hover:animate-[marquee_4s_linear_infinite]"
                          title={capture.filename}
                        >
                          <span className="inline-block">{capture.filename}</span>
                          <span className="inline-block">&nbsp;{capture.filename}</span>
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-3 text-zinc-400">{formatTimestamp(capture.created)}</td>
                    <td className="px-3 py-3 text-zinc-400">{capture.duration != null ? formatDuration(capture.duration) : "—"}</td>
                    <td className="px-3 py-3 text-zinc-400">{formatBytes(capture.size)}</td>
                    <td className="px-3 py-3">
                      <div className="flex gap-1">
                        <Button variant="ghost" onClick={() => setPreviewCapture(capture)}>
                          <Eye className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" onClick={() => void api.openPath(capture.path).catch((error: unknown) => toast.error(String(error)))}>
                          Open
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={() => {
                            setSelectedCapture(capture.path);
                            void api.revealPath(capture.path).catch((error: unknown) => toast.error(String(error)));
                          }}
                          title="Reveal in folder"
                        >
                          <FolderOpen className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          disabled={verifying === capture.path}
                          onClick={() => void verifyCapture(capture.path)}
                        >
                          {verifying === capture.path ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={() => void openNote(capture)}
                          title="Add/edit note"
                        >
                          <StickyNote className="h-4 w-4" />
                        </Button>
                        <Button variant="danger" onClick={() => void deleteCapture(capture.path)}>
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      {previewCapture && (
        <PreviewModal
          capture={previewCapture}
          onClose={() => setPreviewCapture(null)}
          onSnapFrame={(t) => void snapFrame(previewCapture, t)}
          extracting={extracting}
        />
      )}

      {noteCapture && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4"
          onClick={() => setNoteCapture(null)}
        >
          <div
            className="relative w-full max-w-lg rounded-xl border border-zinc-700 bg-zinc-900 p-5"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-4 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <StickyNote className="h-4 w-4 text-amber-400" />
                <span className="text-sm font-medium text-zinc-200">Annotation</span>
              </div>
              <Button variant="ghost" onClick={() => setNoteCapture(null)}>
                <XCircle className="h-4 w-4" />
              </Button>
            </div>
            <div className="mb-2 text-xs text-zinc-500 font-mono">{noteCapture.filename}</div>
            <Textarea
              value={noteText}
              onChange={(e) => setNoteText(e.target.value)}
              placeholder="Add a note about this capture..."
              rows={4}
              className="w-full"
            />
            <div className="mt-3 flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setNoteCapture(null)}>Cancel</Button>
              <Button variant="primary" onClick={() => void saveNote()}>Save Note</Button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
}

function PreviewModal({ capture, onClose, onSnapFrame, extracting }: {
  capture: CaptureInfo;
  onClose: () => void;
  onSnapFrame: (timestamp: number) => void;
  extracting: boolean;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const isVideo = capture.kind === "webcam" || capture.kind === "screen";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4"
      onClick={onClose}
    >
      <div
        className="relative max-h-full max-w-4xl overflow-hidden rounded-xl border border-zinc-700 bg-zinc-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-zinc-800/70 px-4 py-3">
          <span className="font-mono text-xs text-zinc-300">{capture.filename}</span>
          <div className="flex items-center gap-2">
            {isVideo && (
              <Button
                variant="secondary"
                disabled={extracting}
                onClick={() => {
                  const t = videoRef.current?.currentTime ?? 0;
                  onSnapFrame(t);
                }}
              >
                {extracting ? <Loader2 className="h-4 w-4 animate-spin" /> : <Camera className="h-4 w-4" />}
                Snap Frame
              </Button>
            )}
            <Button variant="ghost" onClick={onClose}>
              <XCircle className="h-4 w-4" />
            </Button>
          </div>
        </div>
        <div className="flex max-h-[80vh] items-center justify-center p-4">
          {isVideo ? (
            <video
              ref={videoRef}
              src={convertFileSrc(capture.path)}
              controls
              className="max-h-[70vh] max-w-full"
            />
          ) : (
            <img
              src={convertFileSrc(capture.path)}
              alt={capture.filename}
              className="max-h-[70vh] max-w-full object-contain"
            />
          )}
        </div>
      </div>
    </div>
  );
}

interface AnalysisTabProps {
  captures: CaptureInfo[];
  selectedFile: string | null;
  setSelectedCapture: (path: string) => void;
  prompt: string;
  setPrompt: (prompt: string) => void;
  analysis: AnalysisResult | null;
  setAnalysis: (result: AnalysisResult | null) => void;
  busyAction: string | null;
  setBusyAction: (action: string | null) => void;
}

function AnalysisTab({
  captures,
  selectedFile,
  setSelectedCapture,
  prompt,
  setPrompt,
  analysis,
  setAnalysis,
  busyAction,
  setBusyAction,
}: AnalysisTabProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  const filteredCaptures = captures.filter((c) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return c.filename.toLowerCase().includes(q) || c.created.toLowerCase().includes(q);
  });

  const runAnalysis = async () => {
    try {
      setBusyAction("analysis");
      const result = await api.runAnalysis(selectedFile, prompt);
      setAnalysis(result);
      toast.success("Analysis ready");
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusyAction(null);
    }
  };

  const runMotionAnalysis = async () => {
    try {
      setBusyAction("motionAnalysis");
      const result = await api.analyzeLastMotionClip(prompt);
      setAnalysis(result);
      toast.success("Motion clip analysis ready");
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusyAction(null);
    }
  };

  const copyToClipboard = (text: string) => {
    void navigator.clipboard.writeText(text).then(() => toast.success("Copied to clipboard"));
  };

  return (
    <div className="grid h-full grid-cols-[0.9fr_1.1fr] gap-5">
      <Card className="flex flex-col overflow-hidden p-5">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold text-zinc-50">Percept Engine</h2>
        </div>

        <Input
          placeholder="Search captures..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="mb-3"
        />

        <div className="mb-3 min-h-0 flex-1 overflow-y-auto rounded-md border border-zinc-800 bg-zinc-950">
          {filteredCaptures.length === 0 ? (
            <div className="p-3 text-xs text-zinc-500">No captures found</div>
          ) : (
            filteredCaptures.map((capture) => (
              <button
                key={capture.path}
                onClick={() => { playSfx("button-click-else"); setSelectedCapture(capture.path) }}
                className={`flex w-full items-center gap-2 border-b border-zinc-800/50 px-3 py-2 text-left text-xs hover:bg-zinc-900 ${selectedFile === capture.path ? "bg-emerald-500/10" : ""}`}
              >
                {capture.thumbnail ? (
                  <img src={convertFileSrc(capture.thumbnail)} alt="" className="h-8 w-12 rounded object-cover" />
                ) : (
                  <div className="flex h-8 w-12 items-center justify-center rounded bg-zinc-800">
                    <FileImage className="h-3 w-3 text-zinc-600" />
                  </div>
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-zinc-200">{capture.filename}</div>
                  <div className="text-zinc-500">{formatTimestamp(capture.created)}</div>
                </div>
              </button>
            ))
          )}
        </div>

        <label className="block">
          <span className="mb-2 block text-sm text-zinc-400">Prompt</span>
          <Textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} className="min-h-[80px]" />
        </label>

        <div className="mt-2 flex gap-0.5">
          <Button
            variant="ghost"
            className="text-xs px-1.5 py-0.5"
            onClick={() => setPrompt("Describe any people, movement, or anomalies in this security capture. Be factual, concise, and alert on anything unusual.")}
          >
            Security
          </Button>
          <Button
            variant="ghost"
            className="text-xs px-1.5 py-0.5"
            onClick={() => setPrompt("Summarize timeline of events in this footage.")}
          >
            Timeline
          </Button>
          <Button
            variant="ghost"
            className="text-xs px-1.5 py-0.5"
            onClick={() => setPrompt("Transcribe and analyze any audio if present.")}
          >
            Transcribe
          </Button>
          <Button
            variant="ghost"
            className="text-xs px-1.5 py-0.5"
            onClick={() => setPrompt("Detect and list all people, objects, and vehicles visible. Note counts and locations.")}
          >
            Detect
          </Button>
          <Button
            variant="ghost"
            className="text-xs px-1.5 py-0.5"
            onClick={() => setPrompt("Describe the scene in detail: setting, lighting, weather, time of day, and any anomalies.")}
          >
            Describe
          </Button>
        </div>

        <Button className="mt-3 w-full" variant="primary" disabled={busyAction === "analysis"} onClick={() => void runAnalysis()}>
          {busyAction === "analysis" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Search className="h-4 w-4" />}
          Run defEYE Analysis
        </Button>

        <Button
          className="mt-2 w-full"
          disabled={busyAction === "motionAnalysis"}
          onClick={() => void runMotionAnalysis()}
        >
          {busyAction === "motionAnalysis" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Radar className="h-4 w-4" />}
          Analyze Last Motion Clip
        </Button>
      </Card>

      <Card className="flex flex-col overflow-auto p-5">
        {analysis ? (
          <div className="space-y-4">
            <div className="grid grid-cols-2 gap-3 text-sm">
              <Meta label="File" value={analysis.metadata.file} />
              <Meta label="Captured" value={analysis.metadata.captured} />
              <Meta label="Resolution" value={analysis.metadata.resolution ?? "unknown"} />
              <Meta label="Size" value={formatBytes(analysis.metadata.size)} />
              <Meta label="Confidence" value={`${Math.round(analysis.confidence * 100)}%`} />
              <Meta label="Source" value={analysis.metadata.monitors != null ? `${analysis.metadata.monitors} monitors` : "webcam"} />
            </div>

            {analysis.tags.length > 0 && (
              <div>
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs uppercase text-zinc-500">Tags</span>
                  <Button variant="ghost" className="text-xs px-2 py-0.5" onClick={() => copyToClipboard(analysis.tags.join(", "))}>
                    Copy
                  </Button>
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {analysis.tags.map((tag, i) => (
                    <span key={i} className="rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-0.5 text-xs font-medium text-emerald-300">
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
            )}

            <div>
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs uppercase text-zinc-500">Summary</span>
                <Button variant="ghost" className="text-xs px-2 py-0.5" onClick={() => copyToClipboard(analysis.analysis_text)}>
                  Copy
                </Button>
              </div>
              <pre className="whitespace-pre-wrap rounded-md border border-zinc-800 bg-zinc-900 p-4 font-mono text-xs leading-5 text-zinc-200">
                {analysis.analysis_text}
              </pre>
            </div>

            {analysis.observations.length > 0 && (
              <div>
                <span className="mb-2 block text-xs uppercase text-zinc-500">Key Observations</span>
                <ul className="space-y-1.5">
                  {analysis.observations.map((obs, i) => (
                    <li key={i} className="flex items-start gap-2 text-xs text-zinc-300">
                      <span className="mt-0.5 text-emerald-400">•</span>
                      <span>{obs}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {analysis.raw_response && (
              <div>
                <button
                  onClick={() => { playSfx("button-click-else"); setShowAdvanced(!showAdvanced) }}
                  className="text-xs text-zinc-500 hover:text-zinc-300"
                >
                  {showAdvanced ? "▼" : "▶"} Raw model response
                </button>
                {showAdvanced && (
                  <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-zinc-800 bg-zinc-950 p-3 font-mono text-[10px] leading-4 text-zinc-500">
                    {analysis.raw_response}
                  </pre>
                )}
              </div>
            )}
          </div>
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-zinc-500">
            Select a capture and run analysis to see results.
          </div>
        )}
      </Card>
    </div>
  );
}

function AiTab({ settings, saveSettings }: { settings: Settings; saveSettings: (s: Settings) => Promise<void> }) {
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [testing, setTesting] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  const updateField = async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    await saveSettings({ ...settings, [key]: value });
  };

  const testConnection = async () => {
    setTesting(true);
    try {
      const version = await api.testOllama(settings.ollama_endpoint);
      toast.success(`Ollama connected — version ${version}`);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setTesting(false);
    }
  };

  const detectModels = async () => {
    setDetecting(true);
    try {
      const models = await api.listOllamaModels(settings.ollama_endpoint);
      setOllamaModels(models);
      toast.success(`Found ${models.length} models`);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setDetecting(false);
    }
  };

  return (
    <div className="grid h-full grid-cols-1 gap-5 overflow-auto">
      <Card className="p-5">
        <div className="mb-5 flex items-center gap-2">
          <Brain className="h-5 w-5 text-emerald-400" />
          <h2 className="text-base font-semibold text-zinc-50">defEYE AI Agent Brain</h2>
        </div>

        <div className="mb-5 flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
          <div>
            <div className="text-sm font-medium text-zinc-100">Enable Ollama Integration</div>
            <div className="text-xs text-zinc-500">Connect to a local Ollama instance for vision-based analysis.</div>
          </div>
          <Switch
            checked={settings.ollama_enabled}
            onCheckedChange={(checked) => updateField("ollama_enabled", checked).catch((error: unknown) => toast.error(String(error)))}
          />
        </div>

        <label className="block">
          <span className="mb-2 block text-sm text-zinc-400">Ollama Endpoint</span>
          <div className="flex gap-2">
            <Input
              value={settings.ollama_endpoint}
              onChange={(e) => updateField("ollama_endpoint", e.target.value).catch((error: unknown) => toast.error(String(error)))}
              placeholder="http://localhost:11434"
            />
            <Button
              variant="primary"
              disabled={testing}
              onClick={() => void testConnection()}
            >
              {testing ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
              Test
            </Button>
          </div>
        </label>

        <label className="mt-4 block">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-sm text-zinc-400">Model</span>
            <Button variant="ghost" className="text-xs px-2 py-1" disabled={detecting} onClick={() => void detectModels()}>
              {detecting ? <Loader2 className="h-3 w-3 animate-spin" /> : null}
              Auto-detect
            </Button>
          </div>
          <select
            value={settings.ollama_model}
            onChange={(e) => updateField("ollama_model", e.target.value).catch((error: unknown) => toast.error(String(error)))}
            className="h-9 w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
          >
            <option value="qwen2.5vl:7b">qwen2.5vl:7b</option>
            <option value="qwen2.5vl:3b">qwen2.5vl:3b</option>
            <option value="qwen2.5vl:32b">qwen2.5vl:32b</option>
            <option value="gemma3:12b">gemma3:12b</option>
            <option value="llava">llava</option>
            {ollamaModels.filter((m) => !["qwen2.5vl:7b", "qwen2.5vl:3b", "qwen2.5vl:32b", "gemma3:12b", "llava"].includes(m)).map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <div className="mt-1 text-xs text-zinc-600">
            Pull models with <code className="text-zinc-400">ollama pull &lt;model&gt;</code>. Recommended: qwen2.5vl:7b (fast, compatible with Ollama 0.30+), qwen2.5vl:3b (lightweight). Note: llama3.2-vision is not supported on Ollama 0.30+.
          </div>
        </label>

        <div className="mt-4 flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
          <div>
            <div className="text-sm font-medium text-zinc-100">Auto-analysis on new captures</div>
            <div className="text-xs text-zinc-500">Automatically run analysis with a default prompt when a capture is saved.</div>
          </div>
          <Switch
            checked={settings.auto_analysis_on_capture}
            onCheckedChange={(checked) => updateField("auto_analysis_on_capture", checked).catch((error: unknown) => toast.error(String(error)))}
          />
        </div>

        <button
          onClick={() => { playSfx("button-click-else"); setShowAdvanced(!showAdvanced) }}
          className="mt-4 text-xs text-zinc-500 hover:text-zinc-300"
        >
          {showAdvanced ? "▼" : "▶"} Advanced Settings
        </button>

        {showAdvanced && (
          <div className="mt-3 space-y-4 rounded-md border border-zinc-800 bg-zinc-900/40 p-4">
            <label className="block">
              <span className="mb-2 block text-sm text-zinc-400">Temperature ({settings.ollama_temperature.toFixed(2)})</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={settings.ollama_temperature}
                onChange={(e) => updateField("ollama_temperature", Number(e.target.value)).catch((error: unknown) => toast.error(String(error)))}
                className="w-full"
              />
            </label>

            <label className="block">
              <span className="mb-2 block text-sm text-zinc-400">Max Tokens</span>
              <Input
                type="number"
                min={64}
                max={8192}
                value={settings.ollama_max_tokens}
                onChange={(e) => updateField("ollama_max_tokens", Math.max(64, Number(e.target.value))).catch((error: unknown) => toast.error(String(error)))}
              />
            </label>

            <label className="block">
              <span className="mb-2 block text-sm text-zinc-400">System Prompt</span>
              <Textarea
                value={settings.ollama_system_prompt}
                onChange={(e) => updateField("ollama_system_prompt", e.target.value).catch((error: unknown) => toast.error(String(error)))}
                className="min-h-[100px] font-mono text-xs"
              />
            </label>
          </div>
        )}

        <div className="mt-4 rounded-lg border border-zinc-800/70 bg-zinc-900/40 p-4">
          <div className="flex items-center gap-2 text-xs text-emerald-300">
            <ShieldCheck className="h-3.5 w-3.5" />
            All processing is local. No images leave your machine.
          </div>
        </div>
      </Card>
    </div>
  );
}

interface VoiceCommandsTabProps {
  settings: Settings;
  saveSettings: (settings: Settings) => Promise<void>;
  voiceActive: boolean;
}

const ACTION_LABELS: Record<VoiceAction, string> = {
  start_webcam: "Start Webcam",
  stop_recording: "Stop Recording",
  capture_primary: "Capture Primary",
  capture_all_merged: "Capture All Merged",
  toggle_motion: "Toggle Motion",
  disable_motion: "Disable Motion",
  show_settings: "Toggle Stealth",
  start_screen_recording: "Start Screen Recording",
  stop_all_and_disable_motion: "Stop All & Disable Motion",
  stop_screen_recording: "Stop Screen Recording",
  toggle_stealth: "Toggle Stealth",
  start_timelapse: "Start Time-Lapse",
  stop_timelapse: "Stop Time-Lapse",
  cycle_camera_left: "Cycle Camera Left",
  cycle_camera_right: "Cycle Camera Right",
  capture_region: "Capture Region",
  open_output_folder: "Open Output Folder",
};

const PRESET_THEMES: { name: string; description: string; commands: VoiceCommand[] }[] = [
  {
    name: "Gamer (Default)",
    description: "Video game controller-style commands",
    commands: [
      { phrase: "press start", action: "start_webcam" },
      { phrase: "game over", action: "stop_recording" },
      { phrase: "screenshot", action: "capture_primary" },
      { phrase: "panorama", action: "capture_all_merged" },
      { phrase: "enable radar", action: "toggle_motion" },
      { phrase: "disable radar", action: "disable_motion" },
      { phrase: "stealth mode", action: "toggle_stealth" },
      { phrase: "co-op mode", action: "start_screen_recording" },
      { phrase: "exit co-op", action: "stop_screen_recording" },
      { phrase: "quit to menu", action: "stop_all_and_disable_motion" },
      { phrase: "open inventory", action: "show_settings" },
    ],
  },
  {
    name: "Sentinel",
    description: "Tactical military-themed commands",
    commands: [
      { phrase: "sentinel engage", action: "start_webcam" },
      { phrase: "close the eye", action: "stop_recording" },
      { phrase: "eye capture", action: "capture_primary" },
      { phrase: "wide perimeter", action: "capture_all_merged" },
      { phrase: "activate scan", action: "toggle_motion" },
      { phrase: "stand down", action: "disable_motion" },
      { phrase: "show command center", action: "show_settings" },
      { phrase: "full alert", action: "start_screen_recording" },
      { phrase: "perimeter clear", action: "stop_all_and_disable_motion" },
    ],
  },
  {
    name: "Standard",
    description: "Simple, clear everyday commands",
    commands: [
      { phrase: "start recording", action: "start_webcam" },
      { phrase: "stop recording", action: "stop_recording" },
      { phrase: "take screenshot", action: "capture_primary" },
      { phrase: "capture all screens", action: "capture_all_merged" },
      { phrase: "turn on motion", action: "toggle_motion" },
      { phrase: "turn off motion", action: "disable_motion" },
      { phrase: "open settings", action: "show_settings" },
      { phrase: "record screen", action: "start_screen_recording" },
      { phrase: "stop everything", action: "stop_all_and_disable_motion" },
    ],
  },
  {
    name: "Minimal",
    description: "Just the essential controls",
    commands: [
      { phrase: "start", action: "start_webcam" },
      { phrase: "stop", action: "stop_recording" },
      { phrase: "capture", action: "capture_primary" },
      { phrase: "settings", action: "show_settings" },
    ],
  },
  {
    name: "Cinematic",
    description: "Film director style commands",
    commands: [
      { phrase: "action", action: "start_webcam" },
      { phrase: "cut", action: "stop_recording" },
      { phrase: "still shot", action: "capture_primary" },
      { phrase: "wide shot", action: "capture_all_merged" },
      { phrase: "rolling", action: "start_screen_recording" },
      { phrase: "that's a wrap", action: "stop_all_and_disable_motion" },
      { phrase: "stealth cam", action: "toggle_stealth" },
      { phrase: "time lapse", action: "start_timelapse" },
      { phrase: "cut time lapse", action: "stop_timelapse" },
    ],
  },
  {
    name: "Sci-Fi",
    description: " Futuristic AI interface commands",
    commands: [
      { phrase: "initialize eye", action: "start_webcam" },
      { phrase: "terminate ocular", action: "stop_recording" },
      { phrase: "snapshot", action: "capture_primary" },
      { phrase: "panopticon", action: "capture_all_merged" },
      { phrase: "engage cloak", action: "toggle_stealth" },
      { phrase: "temporal capture", action: "start_timelapse" },
      { phrase: "cease temporal", action: "stop_timelapse" },
      { phrase: "scan perimeter", action: "toggle_motion" },
      { phrase: "shutdown all systems", action: "stop_all_and_disable_motion" },
      { phrase: "switch camera alpha", action: "cycle_camera_left" },
      { phrase: "switch camera beta", action: "cycle_camera_right" },
      { phrase: "open vault", action: "open_output_folder" },
    ],
  },
  {
    name: "Marine Corps",
    description: "Oorah tactical commands",
    commands: [
      { phrase: "weapons hot", action: "start_webcam" },
      { phrase: "cease fire", action: "stop_recording" },
      { phrase: "snap recon", action: "capture_primary" },
      { phrase: "overwatch", action: "capture_all_merged" },
      { phrase: "radar on", action: "toggle_motion" },
      { phrase: "radar off", action: "disable_motion" },
      { phrase: "go dark", action: "toggle_stealth" },
      { phrase: "full barrage", action: "start_screen_recording" },
      { phrase: "hold position", action: "stop_screen_recording" },
      { phrase: "stand down all", action: "stop_all_and_disable_motion" },
      { phrase: "command post", action: "show_settings" },
    ],
  },
  {
    name: "Obscure",
    description: "Cryptic, minimal-effort phrases",
    commands: [
      { phrase: "yo", action: "start_webcam" },
      { phrase: "nah", action: "stop_recording" },
      { phrase: "snap", action: "capture_primary" },
      { phrase: "yoink", action: "capture_all_merged" },
      { phrase: "shh", action: "toggle_stealth" },
      { phrase: "vroom", action: "start_screen_recording" },
      { phrase: "brake", action: "stop_screen_recording" },
      { phrase: "nvm", action: "stop_all_and_disable_motion" },
      { phrase: "home", action: "show_settings" },
      { phrase: "yolo", action: "capture_region" },
      { phrase: "lefty", action: "cycle_camera_left" },
      { phrase: "righty", action: "cycle_camera_right" },
    ],
  },
  {
    name: "Pirate",
    description: "Yarr, sea-faring scallywag commands",
    commands: [
      { phrase: "raise the mast", action: "start_webcam" },
      { phrase: "drop anchor", action: "stop_recording" },
      { phrase: "spyglass", action: "capture_primary" },
      { phrase: "wide horizon", action: "capture_all_merged" },
      { phrase: "man the crow's nest", action: "toggle_motion" },
      { phrase: "stand down the watch", action: "disable_motion" },
      { phrase: "go below deck", action: "toggle_stealth" },
      { phrase: "fire the cannons", action: "start_screen_recording" },
      { phrase: "cease fire", action: "stop_screen_recording" },
      { phrase: "abandon ship", action: "stop_all_and_disable_motion" },
      { phrase: "captain's quarters", action: "show_settings" },
    ],
  },
  {
    name: "Medical",
    description: "Hospital ER code-style commands",
    commands: [
      { phrase: "code blue", action: "start_webcam" },
      { phrase: "code clear", action: "stop_recording" },
      { phrase: "chart snapshot", action: "capture_primary" },
      { phrase: "full scan", action: "capture_all_merged" },
      { phrase: "activate monitors", action: "toggle_motion" },
      { phrase: "discontinue monitors", action: "disable_motion" },
      { phrase: "quiet mode", action: "toggle_stealth" },
      { phrase: "code red", action: "start_screen_recording" },
      { phrase: "code red clear", action: "stop_screen_recording" },
      { phrase: "code black", action: "stop_all_and_disable_motion" },
      { phrase: "nurse station", action: "show_settings" },
    ],
  },
  {
    name: "Chef",
    description: "Kitchen brigade commands",
    commands: [
      { phrase: "fire the oven", action: "start_webcam" },
      { phrase: "turn off the burners", action: "stop_recording" },
      { phrase: "plate it", action: "capture_primary" },
      { phrase: "full spread", action: "capture_all_merged" },
      { phrase: "watch the grill", action: "toggle_motion" },
      { phrase: "stop watching", action: "disable_motion" },
      { phrase: "close the kitchen", action: "toggle_stealth" },
      { phrase: "fire up the second station", action: "start_screen_recording" },
      { phrase: "shut down second station", action: "stop_screen_recording" },
      { phrase: "shut it all down", action: "stop_all_and_disable_motion" },
      { phrase: "call the manager", action: "show_settings" },
    ],
  },
  {
    name: "Aviator",
    description: "Pilot cockpit radio commands",
    commands: [
      { phrase: "cleared for takeoff", action: "start_webcam" },
      { phrase: "cleared to land", action: "stop_recording" },
      { phrase: "mark position", action: "capture_primary" },
      { phrase: "wide area photo", action: "capture_all_merged" },
      { phrase: "activate radar", action: "toggle_motion" },
      { phrase: "radar standby", action: "disable_motion" },
      { phrase: "go dark", action: "toggle_stealth" },
      { phrase: "engage second engine", action: "start_screen_recording" },
      { phrase: "shut down second engine", action: "stop_screen_recording" },
      { phrase: "mayday mayday", action: "stop_all_and_disable_motion" },
      { phrase: "contact tower", action: "show_settings" },
    ],
  },
  {
    name: "Detective",
    description: "Noir investigator commands",
    commands: [
      { phrase: "open the case", action: "start_webcam" },
      { phrase: "case closed", action: "stop_recording" },
      { phrase: "take evidence", action: "capture_primary" },
      { phrase: "crime scene wide", action: "capture_all_merged" },
      { phrase: "start surveillance", action: "toggle_motion" },
      { phrase: "end surveillance", action: "disable_motion" },
      { phrase: "go undercover", action: "toggle_stealth" },
      { phrase: "wire the second room", action: "start_screen_recording" },
      { phrase: "unwire the second room", action: "stop_screen_recording" },
      { phrase: "close all cases", action: "stop_all_and_disable_motion" },
      { phrase: "open the files", action: "show_settings" },
    ],
  },
  {
    name: "Wizard",
    description: "Arcane spell-casting commands",
    commands: [
      { phrase: "open the eye", action: "start_webcam" },
      { phrase: "close the eye", action: "stop_recording" },
      { phrase: "capture vision", action: "capture_primary" },
      { phrase: "all-seeing eye", action: "capture_all_merged" },
      { phrase: "detect motion", action: "toggle_motion" },
      { phrase: "still the air", action: "disable_motion" },
      { phrase: "cast invisibility", action: "toggle_stealth" },
      { phrase: "summon second eye", action: "start_screen_recording" },
      { phrase: "banish second eye", action: "stop_screen_recording" },
      { phrase: "dispel all magic", action: "stop_all_and_disable_motion" },
      { phrase: "open the grimoire", action: "show_settings" },
    ],
  },
  {
    name: "Custom",
    description: "Start from scratch — no preset commands",
    commands: [],
  },
  {
    name: "Custom 2",
    description: "A second blank slate for custom commands",
    commands: [],
  },
];

const DEFAULT_VOICE_COMMANDS: VoiceCommand[] = PRESET_THEMES[0].commands;

function VoiceCommandsTab({ settings, saveSettings, voiceActive }: VoiceCommandsTabProps) {
  const [commandLog, setCommandLog] = useState<{ text: string; time: string }[]>([]);
  const [micDevices, setMicDevices] = useState<AudioInputDevice[]>([]);
  const [transcript, setTranscript] = useState("");
  const [showResetCommandsConfirm, setShowResetCommandsConfirm] = useState(false);
  const [showThemePicker, setShowThemePicker] = useState(false);
  const [dragSource, setDragSource] = useState<number | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);
  const rowHeightRef = useRef(0);
  const [wakeWordDraft, setWakeWordDraft] = useState(settings.voice_wake_word);
  const [wakeWordLocked, setWakeWordLocked] = useState(settings.voice_wake_word.trim().length > 0);

  useEffect(() => {
    setWakeWordDraft(settings.voice_wake_word);
    setWakeWordLocked(settings.voice_wake_word.trim().length > 0);
  }, [settings.voice_wake_word]);

  const updateField = async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    await saveSettings({ ...settings, [key]: value });
  };

  useEffect(() => {
    const unlistenVoiceCmd = listen<string>("voice-command-executed", (event) => {
      const now = new Date().toLocaleTimeString();
      setCommandLog((prev) => [{ text: event.payload, time: now }, ...prev].slice(0, 20));
    });
    const unlistenTranscript = listen<string>("voice-transcript", (event) => {
      setTranscript(event.payload);
    });
    void api.listAudioInputDevices().then(setMicDevices).catch(() => undefined);
    return () => {
      void unlistenVoiceCmd.then((dispose) => dispose());
      void unlistenTranscript.then((dispose) => dispose());
    };
  }, []);

  const isCustom = settings.voice_theme_id === "Custom";
  const isCustom2 = settings.voice_theme_id === "Custom 2";
  const customField = isCustom ? "voice_commands_custom" : isCustom2 ? "voice_commands_custom2" : null;

  const updateCommand = (index: number, field: keyof VoiceCommand, value: string) => {
    const commands = [...settings.voice_commands];
    commands[index] = { ...commands[index], [field]: value };
    const patch = customField ? { [customField]: commands } : {};
    void saveSettings({ ...settings, voice_commands: commands, ...patch }).catch((error: unknown) => toast.error(String(error)));
  };

  const addCommand = () => {
    const commands = [...settings.voice_commands, { phrase: "new command", action: "start_webcam" as VoiceAction }];
    const patch = customField ? { [customField]: commands } : {};
    void saveSettings({ ...settings, voice_commands: commands, ...patch }).catch((error: unknown) => toast.error(String(error)));
  };

  const removeCommand = (index: number) => {
    const commands = settings.voice_commands.filter((_, i) => i !== index);
    const patch = customField ? { [customField]: commands } : {};
    void saveSettings({ ...settings, voice_commands: commands, ...patch }).catch((error: unknown) => toast.error(String(error)));
  };

  const moveCommand = (from: number, to: number) => {
    if (to < 0 || to >= settings.voice_commands.length || from === to) return;
    const commands = [...settings.voice_commands];
    const [item] = commands.splice(from, 1);
    commands.splice(to, 0, item);
    const patch = customField ? { [customField]: commands } : {};
    void saveSettings({ ...settings, voice_commands: commands, ...patch }).catch((error: unknown) => toast.error(String(error)));
  };

  useEffect(() => {
    if (dragSource === null) return;
    const handleMouseUp = () => {
      if (dragOver !== null && dragSource !== dragOver) {
        playSfx("button-click-else");
        moveCommand(dragSource, dragOver);
      }
      setDragSource(null);
      setDragOver(null);
    };
    window.addEventListener("mouseup", handleMouseUp);
    return () => window.removeEventListener("mouseup", handleMouseUp);
  }, [dragSource, dragOver, moveCommand]);

  const getDisplacement = (i: number): number => {
    if (dragSource === null || dragOver === null || dragSource === dragOver) return 0;
    const stride = rowHeightRef.current + 8;
    if (i === dragSource) return (dragOver - dragSource) * stride;
    if (dragSource < dragOver) {
      if (i > dragSource && i <= dragOver) return -stride;
    } else {
      if (i >= dragOver && i < dragSource) return stride;
    }
    return 0;
  };

  return (
    <div className="grid h-full grid-cols-1 gap-5 overflow-auto">
      {/* Main status + start/stop */}
      <Card className="p-5">
        <div className="mb-5 flex items-center gap-2">
          <Mic className="h-5 w-5 text-emerald-400" />
          <h2 className="text-base font-semibold text-zinc-50">Voice Control</h2>
        </div>

        <div className="flex items-center gap-4 py-2">
          {voiceActive ? (
            <Button variant="danger" className="px-6 py-3 text-base shrink-0" onClick={() => void api.toggleVoiceControl().then(() => { toast.info("Voice control DISENGAGED"); }).catch((error: unknown) => toast.error(String(error)))}>
              <MicOff className="h-5 w-5" />
              Stop Listening
            </Button>
          ) : (
            <Button variant="primary" className="px-6 py-3 text-base shrink-0" onClick={() => void api.toggleVoiceControl().then(() => { toast.info("Voice control ENGAGED"); }).catch((error: unknown) => toast.error(String(error)))}>
              <Mic className="h-5 w-5" />
              Start Listening
            </Button>
          )}
          <div className="flex h-12 min-w-0 flex-1 items-center rounded-lg border border-zinc-800/70 bg-zinc-950 px-4">
            {transcript ? (
              <span className="truncate text-sm text-zinc-200">{transcript}</span>
            ) : (
              <span className="text-sm text-zinc-600">{voiceActive ? "Listening…" : "Press Start Listening to begin capturing speech"}</span>
            )}
          </div>
        </div>
      </Card>

      {/* Audio input device + Vosk model path */}
      <Card className="relative p-5">
        <h3 className="mb-5 text-sm font-semibold text-zinc-300">Audio Input & Model</h3>
        <div className="grid gap-6">
          {/* Microphone selection */}
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Microphone Source</span>
            <div className="flex gap-2">
              <select
                value={settings.voice_audio_device}
                onChange={(e) => updateField("voice_audio_device", e.target.value).catch((error: unknown) => toast.error(String(error)))}
                className="h-9 flex-1 rounded-md border border-zinc-800 bg-zinc-950 px-3 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
              >
                <option value="">Default System Microphone</option>
                {micDevices.map((dev) => (
                  <option key={dev.device_id} value={dev.name}>{dev.name}</option>
                ))}
              </select>
              <Button
                variant="ghost"
                className="text-xs px-2 py-1"
                onClick={() => void api.listAudioInputDevices().then(setMicDevices).catch((error: unknown) => toast.error(String(error)))}
              >
                <RefreshCcw className="h-3 w-3" />
                Refresh
              </Button>
            </div>
            <div className="mt-1 text-xs text-zinc-600">
              Select the microphone used for voice recognition. Leave empty for system default.
            </div>
          </label>

          {/* Vosk model path */}
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Vosk Model Path</span>
            <div className="flex gap-2">
              <Input
                value={settings.voice_model_path}
                onChange={(e) => updateField("voice_model_path", e.target.value).catch((error: unknown) => toast.error(String(error)))}
                placeholder="e.g. C:\models\vosk-model-small-en-us-0.15"
              />
              <Button
                variant="ghost"
                className="text-xs px-2 py-1 shrink-0"
                onClick={() => void openDialog({ directory: true }).then((path) => { if (path) void updateField("voice_model_path", path as string); }).catch(() => undefined)}
              >
                <FolderOpen className="h-3 w-3" />
                Browse
              </Button>
            </div>
            <div className="mt-1 text-xs text-zinc-600">
              Download a Vosk model from <span className="font-mono">alphacephei.com/vosk/models</span> and select the extracted folder.
            </div>
          </label>
        </div>
      </Card>

      {/* Recognition settings */}
      <Card className="p-5">
        <h3 className="mb-5 text-sm font-semibold text-zinc-300">Recognition Settings</h3>
        <div className="grid gap-4">
          {/* Confidence threshold */}
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Confidence Threshold ({Math.round(settings.voice_confidence_threshold * 100)}%)</span>
            <input
              type="range"
              min={0.1}
              max={0.99}
              step={0.01}
              value={settings.voice_confidence_threshold}
              onChange={(e) => updateField("voice_confidence_threshold", Number(e.target.value)).catch((error: unknown) => toast.error(String(error)))}
              className="w-full accent-emerald-500"
            />
            <div className="mt-1 flex justify-between text-xs text-zinc-600">
              <span>Lenient — allows misheard words</span>
              <span>Strict — exact match only</span>
            </div>
          </label>

          {/* Wake word */}
          <label className="block">
            <span className="mb-2 block text-sm text-zinc-400">Wake Word (optional)</span>
            <div className="flex gap-2">
              <Input
                value={wakeWordDraft}
                onChange={(e) => {
                  setWakeWordDraft(e.target.value);
                  if (wakeWordLocked) setWakeWordLocked(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    void updateField("voice_wake_word", wakeWordDraft.trim()).catch((error: unknown) => toast.error(String(error)));
                    setWakeWordLocked(true);
                  }
                }}
                placeholder="e.g. defeye — say 'defeye sentinel engage' instead of just 'sentinel engage'"
                className={wakeWordLocked ? "border-emerald-500/60 ring-1 ring-emerald-500/30" : ""}
              />
              <Button
                variant={wakeWordLocked ? "primary" : "ghost"}
                className={`text-xs px-3 py-1.5 shrink-0 transition-all duration-300 ${wakeWordLocked ? "ring-1 ring-emerald-400/50" : ""}`}
                onClick={() => {
                  void updateField("voice_wake_word", wakeWordDraft.trim()).catch((error: unknown) => toast.error(String(error)));
                  setWakeWordLocked(true);
                }}
                title={wakeWordLocked ? "Wake word locked in" : "Click to lock in wake word"}
              >
                {wakeWordLocked ? (
                  <span className="flex items-center gap-1.5">
                    <Lock className="h-3 w-3" />
                    Locked
                  </span>
                ) : (
                  <span className="flex items-center gap-1.5">
                    <Unlock className="h-3 w-3" />
                    Lock In
                  </span>
                )}
              </Button>
            </div>
            <div className="mt-1 text-xs text-zinc-600">
              When set, all commands must be prefixed with the wake word. Leave empty for always-on recognition.
            </div>
            {wakeWordLocked && (
              <div className="mt-2 flex items-center gap-2 text-xs text-emerald-400">
                <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-400" />
                {settings.voice_wake_word
                  ? <>Wake word active — say "{settings.voice_wake_word}" before each command</>
                  : "Always-on recognition — no wake word required"}
              </div>
            )}
          </label>

          {/* TTS feedback */}
          <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
            <div>
              <div className="text-sm font-medium text-zinc-100">Voice Feedback (TTS)</div>
              <div className="text-xs text-zinc-500">Speak confirmation after each command is executed.</div>
            </div>
            <Switch
              checked={settings.voice_feedback}
              onCheckedChange={(checked) => updateField("voice_feedback", checked).catch((error: unknown) => toast.error(String(error)))}
            />
          </div>

          {/* Auto-start */}
          <div className="flex items-center justify-between rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-3">
            <div>
              <div className="text-sm font-medium text-zinc-100">Auto-Start on Launch</div>
              <div className="text-xs text-zinc-500">Begin listening automatically when defEYE starts.</div>
            </div>
            <Switch
              checked={settings.voice_auto_start}
              onCheckedChange={(checked) => updateField("voice_auto_start", checked).catch((error: unknown) => toast.error(String(error)))}
            />
          </div>
        </div>
      </Card>

      {/* Command phrases */}
      <Card className="p-5">
        <div className="mb-5 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-zinc-300">Command Phrases ({settings.voice_commands.length})</h3>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              className="text-xs px-2 py-1"
              onClick={() => setShowThemePicker(!showThemePicker)}
            >
              <Layers className="h-3 w-3" />
              Themes
            </Button>
            <Button
              variant="ghost"
              className="text-xs px-2 py-1"
              onClick={addCommand}
            >
              + Add Command
            </Button>
            <Button
              variant="ghost"
              className="text-xs px-2 py-1 hover:border-amber-600/50 hover:text-amber-300"
              onClick={() => setShowResetCommandsConfirm(true)}
              title="Reset commands to default"
            >
              <RefreshCcw className="h-3 w-3" />
              Reset to Default
            </Button>
          </div>
        </div>

        {showThemePicker && (
          <div className="mb-4 grid grid-cols-2 gap-2 rounded-lg border border-zinc-800/70 bg-zinc-900/60 p-3">
            {PRESET_THEMES.map((theme) => {
              const isCustom = theme.name === "Custom";
              const isCustom2Theme = theme.name === "Custom 2";
              const loadCommands = isCustom2Theme
                ? settings.voice_commands_custom2
                : isCustom
                  ? settings.voice_commands_custom
                  : theme.commands;
              const commandCount = isCustom || isCustom2Theme
                ? (isCustom2Theme ? settings.voice_commands_custom2.length : settings.voice_commands_custom.length)
                : theme.commands.length;
              return (
                <button
                  key={theme.name}
                  onClick={() => {
                    playSfx("button-click-else");
                    const commands = isCustom2Theme
                      ? settings.voice_commands_custom2
                      : isCustom
                        ? settings.voice_commands_custom
                        : theme.commands;
                    void saveSettings({ ...settings, voice_commands: commands, voice_theme_id: theme.name }).catch((error: unknown) => toast.error(String(error)));
                    setShowThemePicker(false);
                    toast.success(`Loaded theme: ${theme.name}`);
                  }}
                  className={`flex items-center justify-between rounded-md border px-3 py-2 text-left transition-colors hover:border-emerald-600/50 hover:bg-zinc-800/60 ${settings.voice_theme_id === theme.name ? "border-emerald-600/50 bg-zinc-800/40" : "border-zinc-800 bg-zinc-900/40"}`}
                >
                  <div>
                    <div className={`text-sm font-medium ${isCustom ? "text-amber-200/80" : isCustom2Theme ? "text-amber-100/70" : "text-zinc-200"}`}>{theme.name}</div>
                    <div className="text-xs text-zinc-500">{theme.description}</div>
                  </div>
                  <span className="text-xs text-zinc-600">{commandCount} commands</span>
                </button>
              );
            })}
          </div>
        )}
        <div className="grid gap-2" style={{ userSelect: dragSource !== null ? "none" : undefined }}>
          {settings.voice_commands.map((cmd, i) => (
            <div
              key={i}
              onMouseEnter={() => {
                if (dragSource !== null && dragSource !== i) setDragOver(i);
              }}
              style={{
                transform: `translateY(${getDisplacement(i)}px)`,
                transition: dragSource !== null ? "transform 200ms ease, opacity 150ms ease" : "none",
                opacity: dragSource === i ? 0.4 : 1,
                zIndex: dragSource === i ? 10 : 1,
                position: "relative",
              }}
              className="flex items-center gap-2 rounded-lg border border-zinc-800/70 bg-zinc-900/60 px-4 py-2.5 hover:border-zinc-700"
            >
              <div className="flex flex-col gap-0.5">
                <button
                  onClick={() => { playSfx("button-click-else"); moveCommand(i, i - 1); }}
                  disabled={i === 0}
                  className="flex h-4 w-4 items-center justify-center text-zinc-500 hover:text-emerald-300 disabled:opacity-30 disabled:hover:text-zinc-500"
                  title="Move up"
                >
                  <ArrowUp className="h-3 w-3" />
                </button>
                <button
                  onClick={() => { playSfx("button-click-else"); moveCommand(i, i + 1); }}
                  disabled={i === settings.voice_commands.length - 1}
                  className="flex h-4 w-4 items-center justify-center text-zinc-500 hover:text-emerald-300 disabled:opacity-30 disabled:hover:text-zinc-500"
                  title="Move down"
                >
                  <ArrowDown className="h-3 w-3" />
                </button>
              </div>
              <div
                onMouseDown={(e) => {
                  const row = e.currentTarget.parentElement as HTMLDivElement;
                  if (row) rowHeightRef.current = row.getBoundingClientRect().height;
                  setDragSource(i);
                  setDragOver(i);
                }}
                className="flex h-8 w-5 cursor-grab items-center justify-center text-zinc-600 hover:text-zinc-400 active:cursor-grabbing"
                title="Drag to reorder"
              >
                <GripVertical className="h-4 w-4" />
              </div>
              <Input
                value={cmd.phrase}
                onChange={(e) => updateCommand(i, "phrase", e.target.value)}
                className="flex-1"
                placeholder="Spoken phrase"
              />
              <select
                value={cmd.action}
                onChange={(e) => updateCommand(i, "action", e.target.value)}
                className="h-9 rounded-md border border-zinc-800 bg-zinc-950 px-2 text-sm text-zinc-100 outline-none transition focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
              >
                {Object.entries(ACTION_LABELS).map(([value, label]) => (
                  <option key={value} value={value}>{label}</option>
                ))}
              </select>
              <button
                onClick={() => { playSfx("button-click-else"); removeCommand(i) }}
                className="flex h-8 w-8 items-center justify-center rounded-md text-zinc-500 hover:bg-red-900/40 hover:text-red-300"
                title="Remove command"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
          {settings.voice_commands.length === 0 && (
            <div className="py-4 text-center text-sm text-zinc-600">
              No voice commands configured. Click "Add Command" to create one.
            </div>
          )}
        </div>
      </Card>

      {showResetCommandsConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setShowResetCommandsConfirm(false)}>
          <div className="mx-4 w-full max-w-sm rounded-xl border border-amber-600/40 bg-zinc-900 p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-amber-900/50">
                <RefreshCcw className="h-5 w-5 text-amber-400" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-zinc-50">Reset Command Phrases?</h3>
                <p className="mt-1 text-sm text-zinc-400">
                  This will reset commands for the current theme ({settings.voice_theme_id || "Gamer (Default)"}). Any custom edits will be lost.
                </p>
              </div>
            </div>
            <div className="mt-5 flex justify-end gap-2">
              <Button onClick={() => setShowResetCommandsConfirm(false)}>
                Cancel
              </Button>
              <Button variant="danger" onClick={() => {
                const currentTheme = PRESET_THEMES.find((t) => t.name === settings.voice_theme_id);
                const resetCommands = currentTheme ? currentTheme.commands : PRESET_THEMES[0].commands;
                const patch = customField ? { [customField]: resetCommands } : {};
                void saveSettings({ ...settings, voice_commands: resetCommands, ...patch }).catch((error: unknown) => toast.error(String(error)));
                setShowResetCommandsConfirm(false);
                toast.success(`Commands reset for theme: ${settings.voice_theme_id || PRESET_THEMES[0].name}`);
              }}>
                <RefreshCcw className="h-4 w-4" />
                Reset to Default
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Live command log */}
      <Card className="p-5">
        <div className="mb-5 flex items-center gap-2">
          <h3 className="text-sm font-semibold text-zinc-300">Command Log</h3>
          {commandLog.length > 0 && (
            <button
              onClick={() => { playSfx("button-click-else"); setCommandLog([]) }}
              className="ml-auto text-xs text-zinc-500 hover:text-zinc-300"
            >
              Clear
            </button>
          )}
        </div>
        {commandLog.length === 0 ? (
          <div className="py-8 text-center text-sm text-zinc-600">
            {voiceActive ? "Say a command — recognized speech will appear here." : "Voice control is not active."}
          </div>
        ) : (
          <div className="max-h-64 space-y-1.5 overflow-auto">
            {commandLog.map((entry, i) => (
              <div key={i} className="flex items-center gap-3 rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-4 py-2 text-sm">
                <span className="font-mono text-xs text-zinc-500">{entry.time}</span>
                <span className="font-medium text-emerald-300">{entry.text}</span>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-zinc-800/70 bg-zinc-900 p-3">
      <div className="text-xs uppercase text-zinc-500">{label}</div>
      <div className="mt-1 break-words text-zinc-100">{value}</div>
    </div>
  );
}

const HOTKEY_ACTIONS: { id: string; label: string }[] = [
  { id: "start_webcam", label: "Start webcam recording" },
  { id: "stop_webcam", label: "Stop webcam recording" },
  { id: "start_screen", label: "Start screen recording" },
  { id: "stop_screen", label: "Stop screen recording" },
  { id: "capture_current", label: "Capture primary screen" },
  { id: "capture_all_merged", label: "Capture all screens (merged)" },
  { id: "toggle_motion_mode", label: "Toggle Motion Mode" },
  { id: "cycle_camera_left", label: "Cycle camera (previous)" },
  { id: "cycle_camera_right", label: "Cycle camera (next)" },
  { id: "capture_region_selector", label: "Select screenshot region" },
  { id: "toggle_timelapse", label: "Toggle Time-Lapse" },
  { id: "kill_defeye", label: "Kill defEYE" },
];

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta"]);

function formatShortcut(shortcut: string): string {
  return shortcut
    .replace(/\+/g, " + ")
    .replace(/ArrowUp/g, "↑")
    .replace(/ArrowDown/g, "↓")
    .replace(/ArrowLeft/g, "←")
    .replace(/ArrowRight/g, "→");
}

function codeToKey(code: string, fallback: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Arrow")) return code;
  if (code === "Space") return "Space";
  if (code === "Enter") return "Enter";
  if (code === "Tab") return "Tab";
  if (code === "Escape") return "Escape";
  if (code === "Backspace") return "Backspace";
  if (code === "Delete") return "Delete";
  if (/^F\d+$/.test(code)) return code;
  if (code.startsWith("Numpad")) return code.slice(6);
  let key = fallback;
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase();
  return key;
}

function eventToShortcut(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  const key = codeToKey(e.code, e.key);
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Super");
  parts.push(key);
  return parts.join("+");
}

function SentinelToy() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef<number>(0);
  const particlesRef = useRef<{ x: number; y: number; vx: number; vy: number; hue: number; size: number; life: number }[]>([]);
  const mouseRef = useRef<{ x: number; y: number; active: boolean }>({ x: 0, y: 0, active: false });
  const hueRef = useRef(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      canvas.width = rect.width * window.devicePixelRatio;
      canvas.height = rect.height * window.devicePixelRatio;
      ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
    };
    resize();
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(canvas);

    const MAX_PARTICLES = 120;

    const spawn = (x: number, y: number, count: number) => {
      for (let i = 0; i < count; i++) {
        const angle = Math.random() * Math.PI * 2;
        const speed = Math.random() * 2 + 0.5;
        if (particlesRef.current.length < MAX_PARTICLES) {
          particlesRef.current.push({
            x,
            y,
            vx: Math.cos(angle) * speed,
            vy: Math.sin(angle) * speed,
            hue: (hueRef.current + Math.random() * 60) % 360,
            size: Math.random() * 3 + 1.5,
            life: 1,
          });
        }
      }
    };

    let lastSpawn = 0;

    const animate = (time: number) => {
      const rect = canvas.getBoundingClientRect();
      const w = rect.width;
      const h = rect.height;

      ctx.clearRect(0, 0, w, h);

      const mouse = mouseRef.current;
      hueRef.current = (hueRef.current + 0.5) % 360;

      if (mouse.active && time - lastSpawn > 30) {
        spawn(mouse.x, mouse.y, 3);
        lastSpawn = time;
      }

      const particles = particlesRef.current;
      for (let i = particles.length - 1; i >= 0; i--) {
        const p = particles[i];

        if (mouse.active) {
          const dx = mouse.x - p.x;
          const dy = mouse.y - p.y;
          const dist = Math.sqrt(dx * dx + dy * dy) + 1;
          const force = Math.min(80 / (dist * dist), 0.5);
          p.vx += (dx / dist) * force;
          p.vy += (dy / dist) * force;
        }

        p.vx *= 0.97;
        p.vy *= 0.97;
        p.x += p.vx;
        p.y += p.vy;
        p.life -= 0.008;

        if (p.life <= 0 || p.x < -10 || p.x > w + 10 || p.y < -10 || p.y > h + 10) {
          particles.splice(i, 1);
          continue;
        }

        for (let j = i + 1; j < particles.length; j++) {
          const p2 = particles[j];
          const ddx = p2.x - p.x;
          const ddy = p2.y - p.y;
          const d = Math.sqrt(ddx * ddx + ddy * ddy);
          if (d < 50) {
            const alpha = (1 - d / 50) * 0.3 * p.life * p2.life;
            ctx.strokeStyle = `hsla(${p.hue}, 80%, 60%, ${alpha})`;
            ctx.lineWidth = 0.8;
            ctx.beginPath();
            ctx.moveTo(p.x, p.y);
            ctx.lineTo(p2.x, p2.y);
            ctx.stroke();
          }
        }

        const radius = p.size * p.life;
        const gradient = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, radius * 3);
        gradient.addColorStop(0, `hsla(${p.hue}, 90%, 65%, ${p.life})`);
        gradient.addColorStop(0.5, `hsla(${p.hue}, 80%, 50%, ${p.life * 0.4})`);
        gradient.addColorStop(1, `hsla(${p.hue}, 70%, 40%, 0)`);
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.arc(p.x, p.y, radius * 3, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = `hsla(${p.hue}, 100%, 80%, ${p.life})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, radius, 0, Math.PI * 2);
        ctx.fill();
      }

      rafRef.current = requestAnimationFrame(animate);
    };

    rafRef.current = requestAnimationFrame(animate);

    const onMouseMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      mouseRef.current = { x: e.clientX - rect.left, y: e.clientY - rect.top, active: true };
    };
    const onMouseLeave = () => {
      mouseRef.current.active = false;
    };
    const onClick = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      spawn(e.clientX - rect.left, e.clientY - rect.top, 20);
    };

    canvas.addEventListener("mousemove", onMouseMove);
    canvas.addEventListener("mouseleave", onMouseLeave);
    canvas.addEventListener("click", onClick);

    return () => {
      cancelAnimationFrame(rafRef.current);
      resizeObserver.disconnect();
      canvas.removeEventListener("mousemove", onMouseMove);
      canvas.removeEventListener("mouseleave", onMouseLeave);
      canvas.removeEventListener("click", onClick);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      className="block w-full h-full absolute inset-0"
      style={{ cursor: "crosshair" }}
    />
  );
}

function AboutTab({ outputDir, settings, setSettings, theme, setTheme }: { outputDir: string; settings: Settings; setSettings: React.Dispatch<React.SetStateAction<Settings>>; theme: ThemeName; setTheme: React.Dispatch<React.SetStateAction<ThemeName>> }) {
  const [stats, setStats] = useState<CaptureStats | null>(null);
  const [capturingId, setCapturingId] = useState<string | null>(null);
  const [showResetConfirm, setShowResetConfirm] = useState(false);

  useEffect(() => {
    void api.getCaptureStats().then(setStats).catch(() => undefined);
  }, [outputDir]);

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  };

  const formatDuration = (totalSecs: number) => {
    if (totalSecs < 1) return "0s";
    const h = Math.floor(totalSecs / 3600);
    const m = Math.floor((totalSecs % 3600) / 60);
    const s = Math.floor(totalSecs % 60);
    if (h > 0) return `${h}h ${m}m ${s}s`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  };

  useEffect(() => {
    if (!capturingId) return;
    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturingId(null);
        return;
      }
      const shortcut = eventToShortcut(e);
      if (!shortcut) return;
      void api
        .updateHotkey(capturingId, shortcut)
        .then(() => {
          setSettings((prev) => ({
            ...prev,
            hotkeys: { ...prev.hotkeys, [capturingId]: shortcut },
          }));
          toast.success(`Hotkey updated: ${formatShortcut(shortcut)}`);
        })
        .catch((error: unknown) => toast.error(String(error)))
        .finally(() => setCapturingId(null));
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [capturingId, setSettings]);

  const handleReset = async () => {
    try {
      await api.resetHotkeys();
      const fresh = await api.getSettings();
      setSettings(fresh);
      toast.success("All hotkeys reset to defaults");
    } catch (error) {
      toast.error(String(error));
    } finally {
      setShowResetConfirm(false);
    }
  };

  return (
    <div className="grid h-full grid-cols-[1fr_0.55fr] gap-5">
      <Card className="p-5 overflow-y-auto">
        <div>
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-zinc-300">Keyboard Shortcuts</h3>
            <button
              onClick={() => { playSfx("button-click-else"); setShowResetConfirm(true) }}
              className="flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs text-zinc-400 transition-colors hover:border-amber-600/50 hover:text-amber-300"
              title="Reset all hotkeys to defaults"
            >
              <RefreshCcw className="h-3 w-3" />
              Reset to Default
            </button>
          </div>
          {/* Stealth shortcut — featured at top, centered */}
          <div className="mb-4 flex justify-center">
            <button
              onClick={() => { playSfx("button-click-else"); setCapturingId("toggle_stealth") }}
              className={`flex items-center gap-3 rounded-lg border px-5 py-3 transition-all duration-200 ${
                capturingId === "toggle_stealth"
                  ? "border-emerald-500 bg-emerald-500/10 animate-pulse"
                  : "border-amber-700/40 bg-amber-950/20 hover:border-amber-600/60 hover:bg-amber-900/30 hover:scale-105 shadow-[0_0_12px_rgba(217,119,6,0.15)]"
              }`}
              title={capturingId === "toggle_stealth" ? "Press a key combination… (Esc to cancel)" : "Click to rebind"}
            >
              <EyeOff className="h-5 w-5 text-amber-400" />
              <span className="text-sm font-semibold text-amber-200">Toggle Stealth (Show/Hide)</span>
              {capturingId === "toggle_stealth" ? (
                <span className="font-mono text-xs text-emerald-300 animate-pulse">Press keys…</span>
              ) : (
                <kbd className="rounded border border-amber-700/50 bg-amber-900/30 px-2 py-1 font-mono text-xs text-amber-200">{formatShortcut(settings.hotkeys.toggle_stealth)}</kbd>
              )}
            </button>
          </div>

          <div className="grid grid-cols-2 gap-x-4 gap-y-1.5">
            {HOTKEY_ACTIONS.map((hk) => {
              const currentKey = settings.hotkeys[hk.id as keyof HotkeySettings];
              const isCapturing = capturingId === hk.id;
              const isKill = hk.id === "kill_defeye";
              return (
                <button
                  key={hk.id}
                  onClick={() => { playSfx("button-click-else"); setCapturingId(hk.id) }}
                  className={`flex items-center justify-between rounded-md border px-2.5 py-1.5 text-left transition-colors ${
                    isCapturing
                      ? "border-emerald-500 bg-emerald-500/10"
                      : isKill
                        ? "border-red-900/50 bg-red-950/30 hover:border-red-700/70 hover:bg-red-900/30"
                        : "border-zinc-800 bg-zinc-900/40 hover:border-zinc-600 hover:bg-zinc-800/60"
                  }`}
                  title={isCapturing ? "Press a key combination… (Esc to cancel)" : "Click to rebind"}
                >
                  <span className={`text-xs ${isKill ? "text-red-300/90 font-semibold" : "text-zinc-400"}`}>{hk.label}</span>
                  {isCapturing ? (
                    <span className="font-mono text-[10px] text-emerald-300 animate-pulse">Press keys…</span>
                  ) : (
                    <kbd className={`rounded px-1.5 py-0.5 font-mono text-[10px] ${
                      isKill
                        ? "border border-red-800/50 bg-red-900/20 text-red-300/80"
                        : "border border-zinc-700 bg-zinc-800 text-zinc-300"
                    }`}>{formatShortcut(currentKey)}</kbd>
                  )}
                </button>
              );
            })}
          </div>
        </div>

        {/* Capture statistics */}
        {stats && (
          <div className="sentinel-fade-in mt-6">
            <h3 className="mb-4 text-sm font-semibold text-zinc-300">Capture Statistics</h3>
            <div className="grid grid-cols-3 gap-x-4 gap-y-2">
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Total captures</div>
                <div className="text-lg font-semibold text-zinc-100">{stats.total_count}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Total size</div>
                <div className="text-lg font-semibold text-zinc-100">{formatSize(stats.total_size_bytes)}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Video duration</div>
                <div className="text-lg font-semibold text-sky-300">{formatDuration(stats.total_video_duration_secs)}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Webcam recordings</div>
                <div className="text-sm font-medium text-emerald-300">{stats.webcam_count}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Screen recordings</div>
                <div className="text-sm font-medium text-emerald-300">{stats.screen_count}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Screenshots</div>
                <div className="text-sm font-medium text-emerald-300">{stats.image_count}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Multi-camera</div>
                <div className="text-sm font-medium text-emerald-300">{stats.multi_count}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Largest capture</div>
                <div className="text-sm font-medium text-amber-300">{formatSize(stats.largest_capture_bytes)}</div>
              </div>
              <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                <div className="text-xs text-zinc-500">Video ratio</div>
                <div className="text-sm font-medium text-violet-300">{stats.video_percentage.toFixed(1)}%</div>
              </div>
              {stats.timelapse_count > 0 && (
                <div className="rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                  <div className="text-xs text-zinc-500">Time-lapse frames</div>
                  <div className="text-sm font-medium text-violet-300">{stats.timelapse_count}</div>
                </div>
              )}
              {stats.oldest && (
                <div className="col-span-3 rounded-lg border border-zinc-800/70 bg-zinc-900/40 px-3 py-2">
                  <div className="text-xs text-zinc-500">Range</div>
                  <div className="text-xs text-zinc-300">{stats.oldest} → {stats.newest}</div>
                </div>
              )}
            </div>
          </div>
        )}
      </Card>

      <Card className="p-5 flex flex-col min-h-0">
        <h2 className="mb-4 text-base font-semibold text-zinc-50">System</h2>
        <div className="mb-3">
          <div className="mb-1.5 text-xs font-medium text-zinc-500">Color Theme</div>
          <div className="grid grid-cols-3 gap-2">
            {themes.map((t) => {
              const isActive = theme === t.id;
              return (
                <button
                  key={t.id}
                  onClick={() => { playSfx("button-click-else"); setTheme(t.id); }}
                  className={`group relative flex flex-col items-center gap-1.5 rounded-lg border px-2 py-2.5 text-center transition-all duration-200 ${
                    isActive
                      ? "border-emerald-500/60 bg-emerald-500/10 shadow-[0_0_12px_rgba(16,185,129,0.2)]"
                      : "border-zinc-800 bg-zinc-900/40 hover:border-zinc-600 hover:bg-zinc-800/60 hover:-translate-y-px"
                  }`}
                  title={t.description}
                >
                  <span className={`h-5 w-5 rounded-full border-2 transition-all ${isActive ? "border-emerald-400" : "border-zinc-600 group-hover:border-zinc-500"}`} style={{
                    background: t.id === "sentinel"
                      ? "linear-gradient(135deg, rgb(16 185 129), rgb(6 78 59))"
                      : t.id === "amber"
                        ? "linear-gradient(135deg, rgb(251 191 36), rgb(120 53 15))"
                        : "linear-gradient(135deg, rgb(6 182 212), rgb(139 92 246))",
                    boxShadow: isActive ? "0 0 8px rgba(16,185,129,0.4)" : "none",
                  }} />
                  <span className={`text-[11px] font-medium leading-tight ${isActive ? "text-emerald-300" : "text-zinc-400"}`}>{t.label}</span>
                </button>
              );
            })}
          </div>
        </div>
        <div className="grid gap-3 shrink-0">
          <Button onClick={() => void api.openPath(outputDir).catch((error: unknown) => toast.error(String(error)))}>
            <FolderOpen className="h-4 w-4" />
            Open Output Folder
          </Button>
          <Button onClick={() => void api.openPath("defeye://config-folder").catch((error: unknown) => toast.error(String(error)))}>
            <FolderOpen className="h-4 w-4" />
            Open Config Folder
          </Button>
          <Button
            onClick={() => void api.setSystemTrayEnabled(true).then(() => { setSettings((prev) => ({ ...prev, system_tray_enabled: true })); toast.success("System tray icon restored"); }).catch((error: unknown) => toast.error(String(error)))}
            disabled={settings.system_tray_enabled}
            className={settings.system_tray_enabled ? "opacity-50 cursor-not-allowed" : ""}
          >
            <Eye className="h-4 w-4" />
            {settings.system_tray_enabled ? "System Tray Active" : "Restore System Tray"}
          </Button>
          <Button variant="danger" sfx="terminate" onClick={() => void api.exitApp().catch((error: unknown) => toast.error(String(error)))}>
            <XCircle className="h-4 w-4" />
            Kill defEYE
          </Button>
        </div>

        <div className="mt-5 flex-1 min-h-0 relative">
          <SentinelToy />
        </div>
      </Card>

      {showResetConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setShowResetConfirm(false)}>
          <div className="mx-4 w-full max-w-sm rounded-xl border border-amber-600/40 bg-zinc-900 p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-start gap-3">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-amber-900/50">
                <RefreshCcw className="h-5 w-5 text-amber-400" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-zinc-50">Reset All Hotkeys?</h3>
                <p className="mt-1 text-sm text-zinc-400">
                  This will restore all keyboard shortcuts to their default values. Any custom bindings will be lost.
                </p>
              </div>
            </div>
            <div className="mt-5 flex justify-end gap-2">
              <Button onClick={() => setShowResetConfirm(false)}>
                Cancel
              </Button>
              <Button variant="danger" onClick={() => void handleReset()}>
                <RefreshCcw className="h-4 w-4" />
                Reset to Default
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function RegionSelector() {
  const [dragging, setDragging] = useState(false);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [end, setEnd] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    document.documentElement.classList.add("region-selector-html");
    return () => {
      document.documentElement.classList.remove("region-selector-html");
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.closeRegionSelector();
    };
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      void api.closeRegionSelector();
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("contextmenu", onContextMenu);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("contextmenu", onContextMenu);
    };
  }, []);

  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      setDragging(true);
      setStart({ x: e.clientX, y: e.clientY });
      setEnd({ x: e.clientX, y: e.clientY });
    };
    const onMouseMove = (e: MouseEvent) => {
      if (dragging) setEnd({ x: e.clientX, y: e.clientY });
    };
    const onMouseUp = (e: MouseEvent) => {
      if (!dragging || !start) {
        setDragging(false);
        return;
      }
      const x = Math.min(start.x, e.clientX);
      const y = Math.min(start.y, e.clientY);
      const width = Math.abs(e.clientX - start.x);
      const height = Math.abs(e.clientY - start.y);
      setDragging(false);
      if (width > 5 && height > 5) {
        void emit("region-selected", { x, y, width, height }).then(() => {
          void api.closeRegionSelector();
        });
      } else {
        void api.closeRegionSelector();
      }
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
    };
  }, [dragging, start]);

  const rect = (() => {
    if (!start || !end) return null;
    return {
      left: Math.min(start.x, end.x),
      top: Math.min(start.y, end.y),
      width: Math.abs(end.x - start.x),
      height: Math.abs(end.y - start.y),
    };
  })();

  return (
    <div className="fixed inset-0 cursor-crosshair overflow-hidden" style={{ background: "rgba(0,0,0,0.6)" }}>
      {rect && rect.width > 0 && rect.height > 0 && (
        <div
          className="fixed border-2 border-emerald-400"
          style={{
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
            boxShadow: "0 0 0 9999px rgba(0,0,0,0.6)",
          }}
        >
          <div className="absolute left-1 top-1 rounded bg-emerald-500/80 px-1.5 py-0.5 text-[10px] font-medium text-white">
            {Math.round(rect.width)} x {Math.round(rect.height)}
          </div>
        </div>
      )}
      <div className="pointer-events-none fixed left-1/2 top-6 -translate-x-1/2 rounded-lg bg-zinc-900/90 px-4 py-2 text-sm text-zinc-200 shadow-lg">
        Drag to select region — Esc or right-click to cancel
      </div>
    </div>
  );
}


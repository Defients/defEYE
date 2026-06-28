export type HudCorner = "top_left" | "top_right" | "bottom_left" | "bottom_right" | "hidden";

export type MultiCameraMode = "single" | "multi" | "quick_switch";
export type ScreenshotRegionMode = "full" | "primary" | "custom";
export type ScreenCaptureMode = "all_monitors" | "specific_monitor";
export type RecordingPreset = "ultra" | "high" | "medium" | "low" | "custom";
export type WatermarkPosition = "top_left" | "top_right" | "bottom_left" | "bottom_right" | "center" | "custom";
export type TimeLapseTarget = "screen" | "webcam" | "both";

export interface HotkeySettings {
  start_webcam: string;
  stop_webcam: string;
  start_screen: string;
  stop_screen: string;
  capture_current: string;
  capture_all_merged: string;
  toggle_motion_mode: string;
  cycle_camera_left: string;
  cycle_camera_right: string;
  capture_region_selector: string;
  toggle_stealth: string;
  toggle_timelapse: string;
  kill_defeye: string;
}

export interface CustomRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Settings {
  camera_device: string;
  manual_camera_device: string;
  crf: number;
  include_audio: boolean;
  screen_fps: number;
  screen_crf: number;
  output_dir: string;
  primary_monitor_id: string;
  hud_corner: HudCorner;
  hud_minimal: boolean;
  motion_mode_enabled: boolean;
  motion_sensitivity: number;
  motion_cooldown_seconds: number;
  auto_record_on_motion: boolean;
  motion_triggers_screen: boolean;
  motion_post_record_seconds: number;
  motion_min_record_seconds: number;
  // Audio control
  webcam_audio_device: string;
  webcam_audio_enabled: boolean;
  screen_audio_device: string;
  screen_audio_enabled: boolean;
  // Screen recording target
  screen_capture_mode: ScreenCaptureMode;
  screen_monitor_id: string;
  // Recording quality preset
  recording_preset: RecordingPreset;
  // Auto-stop max recording duration (0 = unlimited)
  max_recording_duration: number;
  // Auto-restart recording after max duration is reached
  auto_restart_recording: boolean;
  // Multi-camera
  multi_camera_devices: string[];
  multi_camera_mode: MultiCameraMode;
  // Region selection
  screenshot_region_mode: ScreenshotRegionMode;
  custom_region: CustomRegion;
  // Evidence hardening
  watermark_enabled: boolean;
  watermark_image_enabled: boolean;
  watermark_image_path: string;
  watermark_opacity: number;
  watermark_scale: number;
  watermark_position: WatermarkPosition;
  watermark_x: number;
  watermark_y: number;
  embed_metadata: boolean;
  integrity_check: boolean;
  // AI / Ollama settings
  ollama_enabled: boolean;
  ollama_endpoint: string;
  ollama_model: string;
  auto_analysis_on_capture: boolean;
  ollama_temperature: number;
  ollama_max_tokens: number;
  ollama_system_prompt: string;
  // Sentinel Watchdog
  watchdog_enabled: boolean;
  // Disk Sentinel
  disk_threshold_mb: number;
  // Time-Lapse
  timelapse_interval_seconds: number;
  timelapse_target: TimeLapseTarget;
  // Hotkey bindings
  hotkeys: HotkeySettings;
  // Voice control
  voice_control_enabled: boolean;
  voice_audio_device: string;
  voice_wake_word: string;
  voice_confidence_threshold: number;
  voice_model_path: string;
  voice_commands: VoiceCommand[];
  voice_theme_id: string;
  voice_commands_custom: VoiceCommand[];
  voice_commands_custom2: VoiceCommand[];
  voice_auto_start: boolean;
  voice_feedback: boolean;
  system_tray_enabled: boolean;
}

export type VoiceAction =
  | "start_webcam"
  | "stop_recording"
  | "capture_primary"
  | "capture_all_merged"
  | "toggle_motion"
  | "disable_motion"
  | "show_settings"
  | "start_screen_recording"
  | "stop_all_and_disable_motion"
  | "stop_screen_recording"
  | "toggle_stealth"
  | "start_timelapse"
  | "stop_timelapse"
  | "cycle_camera_left"
  | "cycle_camera_right"
  | "capture_region"
  | "open_output_folder";

export interface VoiceCommand {
  phrase: string;
  action: VoiceAction;
}

export type CaptureKind = "webcam" | "screen" | "current" | "merged" | "timelapse";

export interface AudioDevice {
  name: string;
  index: number;
}

export interface IntegrityResult {
  verified: boolean;
  stored_hash: string | null;
  actual_hash: string | null;
  message: string;
}

export interface CaptureInfo {
  path: string;
  filename: string;
  kind: CaptureKind;
  size: number;
  created: string;
  thumbnail: string | null;
  has_watermark: boolean;
  has_integrity: boolean;
  has_note: boolean;
  session?: string;
  duration?: number;
}

export interface MonitorInfo {
  id: string;
  name: string;
  friendly_name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface AnalysisMetadata {
  file: string;
  captured: string;
  monitors: number | null;
  resolution: string | null;
  size: number;
  prompt: string;
}

export interface AnalysisResult {
  metadata: AnalysisMetadata;
  analysis_text: string;
  confidence: number;
  tags: string[];
  observations: string[];
  raw_response: string;
}

export interface StatusPayload {
  is_recording: boolean;
  recording_kind: "idle" | "webcam" | "screen" | "mixed" | "finalizing" | string;
  webcam_active: boolean;
  screen_active: boolean;
  multi_active: boolean;
  finalizing: boolean;
}

export interface FileCreatedPayload {
  path: string;
}

export interface MotionStatusPayload {
  motion_mode_enabled: boolean;
  motion_active: boolean;
  last_detection: string | null;
}

export interface CaptureStats {
  total_count: number;
  total_size_bytes: number;
  webcam_count: number;
  screen_count: number;
  multi_count: number;
  image_count: number;
  timelapse_count: number;
  oldest: string | null;
  newest: string | null;
  total_video_duration_secs: number;
  largest_capture_bytes: number;
  video_percentage: number;
}

export interface MotionDetectedPayload {
  timestamp: string;
  message: string;
}

export interface DiskInfoPayload {
  free_bytes: number;
  total_bytes: number;
  free_mb: number;
  threshold_mb: number;
  warning: boolean;
}

export interface CaptureNotePayload {
  path: string;
  note: string | null;
}

export interface TimelapseStatusPayload {
  active: boolean;
  interval_seconds: number;
  target: string;
  last_capture: string | null;
}

export interface VoiceStatusPayload {
  active: boolean;
  status: string;
  last_command: string | null;
  last_command_time: string | null;
}

export interface AudioInputDevice {
  name: string;
  device_id: string;
}

use std::{
    collections::BTreeSet,
    fs,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Local};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use image::{GenericImage, ImageBuffer, ImageFormat, Rgba};
use parking_lot::Mutex;
use std::sync::Mutex as StdMutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_shell::ShellExt;
mod vosk_dynamic;
use vosk_dynamic::{Model, Recognizer};
use xcap::Monitor;

const SETTINGS_FILE: &str = "defeye_settings.json";
const HUD_LABEL: &str = "hud";
const MAIN_LABEL: &str = "main";
const REGION_SELECTOR_LABEL: &str = "region_selector";

const FFMPEG_STARTUP_GRACE: Duration = Duration::from_millis(350);
const FFMPEG_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FILE_FINALIZE_TIMEOUT: Duration = Duration::from_secs(8);
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);
const DISK_CHECK_INTERVAL: Duration = Duration::from_secs(10);
const TIMELAPSE_MIN_INTERVAL: u32 = 5;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run a closure on a dedicated thread with COM initialized.
/// Tauri's async runtime (tokio) may already initialize COM on worker threads
/// with a different apartment model, causing CoInitializeEx to fail with
/// RPC_E_CHANGED_MODE. Spawning a fresh thread ensures COM is properly set up
/// for xcap's DXGI/WGC calls.
fn run_with_com<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    #[cfg(target_os = "windows")]
    {
        thread::spawn(|| {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let com_ok = hr.is_ok();
            let result = f();
            if com_ok {
                // Do NOT call CoUninitialize — xcap/DXGI may still have async COM
                // resources being released. Let the thread exit clean up COM.
            }
            result
        })
        .join()
        .map_err(|_| anyhow!("COM worker thread panicked"))
        .and_then(|r| r)
    }
    #[cfg(not(target_os = "windows"))]
    {
        f()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub camera_device: String,
    pub manual_camera_device: String,
    pub crf: u8,
    pub include_audio: bool,
    pub screen_fps: u8,
    pub screen_crf: u8,
    pub output_dir: PathBuf,
    pub primary_monitor_id: String,
    pub hud_corner: HudCorner,
    pub hud_minimal: bool,
    pub motion_mode_enabled: bool,
    pub motion_sensitivity: u8,
    pub motion_cooldown_seconds: u32,
    pub auto_record_on_motion: bool,
    pub motion_triggers_screen: bool,
    pub motion_post_record_seconds: u32,
    pub motion_min_record_seconds: u32,
    // Audio control
    pub webcam_audio_device: String,
    pub webcam_audio_enabled: bool,
    pub screen_audio_device: String,
    pub screen_audio_enabled: bool,
    // Screen recording target
    pub screen_capture_mode: ScreenCaptureMode,
    pub screen_monitor_id: String,
    // Recording quality preset
    pub recording_preset: RecordingPreset,
    // Auto-stop max recording duration in seconds (0 = unlimited)
    pub max_recording_duration: u32,
    // Auto-restart recording after max duration is reached
    pub auto_restart_recording: bool,
    // Multi-camera
    pub multi_camera_devices: Vec<String>,
    pub multi_camera_mode: MultiCameraMode,
    // Region selection
    pub screenshot_region_mode: ScreenshotRegionMode,
    pub custom_region: CustomRegion,
    // Evidence hardening
    pub watermark_enabled: bool,
    pub watermark_image_enabled: bool,
    pub watermark_image_path: String,
    pub watermark_opacity: f32,
    pub watermark_scale: f32,
    pub watermark_position: WatermarkPosition,
    pub watermark_x: i32,
    pub watermark_y: i32,
    pub embed_metadata: bool,
    pub integrity_check: bool,
    // AI / Ollama settings
    pub ollama_enabled: bool,
    pub ollama_endpoint: String,
    pub ollama_model: String,
    pub auto_analysis_on_capture: bool,
    pub ollama_temperature: f32,
    pub ollama_max_tokens: u32,
    pub ollama_system_prompt: String,
    // Sentinel Watchdog — auto-recover ffmpeg crashes
    pub watchdog_enabled: bool,
    // Disk Sentinel — auto-stop when disk space is low
    pub disk_threshold_mb: u32,
    // Time-Lapse capture
    pub timelapse_interval_seconds: u32,
    pub timelapse_target: TimeLapseTarget,
    // Hotkey bindings
    pub hotkeys: HotkeySettings,
    // Voice control
    pub voice_control_enabled: bool,
    pub voice_audio_device: String,
    pub voice_wake_word: String,
    pub voice_confidence_threshold: f32,
    pub voice_model_path: String,
    pub voice_commands: Vec<VoiceCommand>,
    pub voice_theme_id: String,
    pub voice_commands_custom: Vec<VoiceCommand>,
    pub voice_commands_custom2: Vec<VoiceCommand>,
    pub voice_auto_start: bool,
    pub voice_feedback: bool,
    pub system_tray_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            camera_device: String::new(),
            manual_camera_device: String::new(),
            crf: 23,
            include_audio: true,
            screen_fps: 15,
            screen_crf: 23,
            output_dir: default_output_dir(),
            primary_monitor_id: String::new(),
            hud_corner: HudCorner::TopRight,
            hud_minimal: false,
            motion_mode_enabled: false,
            motion_sensitivity: 50,
            motion_cooldown_seconds: 30,
            auto_record_on_motion: true,
            motion_triggers_screen: false,
            motion_post_record_seconds: 15,
            motion_min_record_seconds: 5,
            webcam_audio_device: String::new(),
            webcam_audio_enabled: true,
            screen_audio_device: String::new(),
            screen_audio_enabled: false,
            screen_capture_mode: ScreenCaptureMode::AllMonitors,
            screen_monitor_id: String::new(),
            recording_preset: RecordingPreset::Medium,
            max_recording_duration: 0,
            auto_restart_recording: true,
            multi_camera_devices: Vec::new(),
            multi_camera_mode: MultiCameraMode::Single,
            screenshot_region_mode: ScreenshotRegionMode::Full,
            custom_region: CustomRegion::default(),
            watermark_enabled: false,
            watermark_image_enabled: false,
            watermark_image_path: String::new(),
            watermark_opacity: 0.5,
            watermark_scale: 0.1,
            watermark_position: WatermarkPosition::BottomRight,
            watermark_x: 10,
            watermark_y: 10,
            embed_metadata: false,
            integrity_check: false,
            ollama_enabled: false,
            ollama_endpoint: "http://localhost:11434".to_string(),
            ollama_model: "qwen2.5vl:7b".to_string(),
            auto_analysis_on_capture: false,
            ollama_temperature: 0.3,
            ollama_max_tokens: 1024,
            ollama_system_prompt: "You are defEYE, the unblinking AI sentinel in Deffy's ACU. Analyze security captures factually, concisely, and alert on anomalies/people/movement. Output structured JSON: {summary, tags: [], confidence: 0-1, key_observations}".to_string(),
            watchdog_enabled: true,
            disk_threshold_mb: 1000,
            timelapse_interval_seconds: 5,
            timelapse_target: TimeLapseTarget::Screen,
            hotkeys: HotkeySettings::default(),
            voice_control_enabled: false,
            voice_audio_device: String::new(),
            voice_wake_word: String::new(),
            voice_confidence_threshold: 0.65,
            voice_model_path: String::new(),
            voice_commands: vec![
                VoiceCommand { phrase: "sentinel engage".to_string(), action: VoiceAction::StartWebcam },
                VoiceCommand { phrase: "close the eye".to_string(), action: VoiceAction::StopRecording },
                VoiceCommand { phrase: "eye capture".to_string(), action: VoiceAction::CapturePrimary },
                VoiceCommand { phrase: "wide perimeter".to_string(), action: VoiceAction::CaptureAllMerged },
                VoiceCommand { phrase: "activate scan".to_string(), action: VoiceAction::ToggleMotion },
                VoiceCommand { phrase: "stand down".to_string(), action: VoiceAction::DisableMotion },
                VoiceCommand { phrase: "show command center".to_string(), action: VoiceAction::ShowSettings },
                VoiceCommand { phrase: "full alert".to_string(), action: VoiceAction::StartScreenRecording },
                VoiceCommand { phrase: "perimeter clear".to_string(), action: VoiceAction::StopAllAndDisableMotion },
            ],
            voice_theme_id: String::new(),
            voice_commands_custom: vec![],
            voice_commands_custom2: vec![],
            voice_auto_start: false,
            voice_feedback: true,
            system_tray_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MultiCameraMode {
    Single,
    Multi,
    QuickSwitch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotRegionMode {
    Full,
    Primary,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenCaptureMode {
    AllMonitors,
    SpecificMonitor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingPreset {
    Ultra,
    High,
    Medium,
    Low,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimeLapseTarget {
    Screen,
    Webcam,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CustomRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for CustomRegion {
    fn default() -> Self {
        Self { x: 0, y: 0, width: 1920, height: 1080 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HotkeySettings {
    pub start_webcam: String,
    pub stop_webcam: String,
    pub start_screen: String,
    pub stop_screen: String,
    pub capture_current: String,
    pub capture_all_merged: String,
    pub toggle_motion_mode: String,
    pub cycle_camera_left: String,
    pub cycle_camera_right: String,
    pub capture_region_selector: String,
    pub toggle_stealth: String,
    pub toggle_timelapse: String,
    pub kill_defeye: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            start_webcam: "Shift+ArrowUp".to_string(),
            stop_webcam: "Shift+ArrowDown".to_string(),
            start_screen: "Ctrl+ArrowUp".to_string(),
            stop_screen: "Ctrl+ArrowDown".to_string(),
            capture_current: "Ctrl+ArrowLeft".to_string(),
            capture_all_merged: "Ctrl+ArrowRight".to_string(),
            toggle_motion_mode: "Ctrl+Shift+ArrowUp".to_string(),
            cycle_camera_left: "Ctrl+Shift+ArrowLeft".to_string(),
            cycle_camera_right: "Ctrl+Shift+ArrowRight".to_string(),
            capture_region_selector: "Ctrl+Alt+ArrowUp".to_string(),
            toggle_stealth: "Ctrl+Shift+ArrowDown".to_string(),
            toggle_timelapse: "Ctrl+Alt+ArrowRight".to_string(),
            kill_defeye: "Ctrl+Alt+ArrowDown".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HudCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Hidden,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureInfo {
    pub path: String,
    pub filename: String,
    pub kind: String,
    pub size: u64,
    pub created: String,
    pub thumbnail: Option<String>,
    pub has_watermark: bool,
    pub has_integrity: bool,
    pub has_note: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityResult {
    pub verified: bool,
    pub stored_hash: Option<String>,
    pub actual_hash: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub friendly_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureStats {
    pub total_count: usize,
    pub total_size_bytes: u64,
    pub webcam_count: usize,
    pub screen_count: usize,
    pub multi_count: usize,
    pub image_count: usize,
    pub timelapse_count: usize,
    pub oldest: Option<String>,
    pub newest: Option<String>,
    pub total_video_duration_secs: f64,
    pub largest_capture_bytes: u64,
    pub video_percentage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisMetadata {
    pub file: String,
    pub captured: String,
    pub monitors: Option<usize>,
    pub resolution: Option<String>,
    pub size: u64,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub metadata: AnalysisMetadata,
    pub analysis_text: String,
    pub confidence: f32,
    pub tags: Vec<String>,
    pub observations: Vec<String>,
    pub raw_response: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub is_recording: bool,
    pub recording_kind: String,
    pub webcam_active: bool,
    pub screen_active: bool,
    pub multi_active: bool,
    pub finalizing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotionStatusPayload {
    pub motion_mode_enabled: bool,
    pub motion_active: bool,
    pub last_detection: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MotionDetectedPayload {
    timestamp: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct FileCreatedPayload {
    path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfoPayload {
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub free_mb: u64,
    pub threshold_mb: u32,
    pub warning: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureNotePayload {
    pub path: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelapseStatusPayload {
    pub active: bool,
    pub interval_seconds: u32,
    pub target: String,
    pub last_capture: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceStatusPayload {
    pub active: bool,
    pub status: String,
    pub last_command: Option<String>,
    pub last_command_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAction {
    StartWebcam,
    StopRecording,
    CapturePrimary,
    CaptureAllMerged,
    ToggleMotion,
    DisableMotion,
    ShowSettings,
    StartScreenRecording,
    StopAllAndDisableMotion,
    StopScreenRecording,
    ToggleStealth,
    StartTimelapse,
    StopTimelapse,
    CycleCameraLeft,
    CycleCameraRight,
    CaptureRegion,
    OpenOutputFolder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceCommand {
    pub phrase: String,
    pub action: VoiceAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioInputDevice {
    pub name: String,
    pub device_id: String,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub is_recording: AtomicBool,
    pub recording_kind: Mutex<String>,
    pub recording_child: Arc<Mutex<Option<std::process::Child>>>,
    pub screen_recording_child: Arc<Mutex<Option<std::process::Child>>>,
    pub multi_recording_children: Arc<Mutex<Vec<std::process::Child>>>,
    pub active_camera_index: AtomicUsize,
    pub recording_output_path: Arc<Mutex<Option<PathBuf>>>,
    pub screen_recording_output_path: Arc<Mutex<Option<PathBuf>>>,
    pub multi_recording_output_paths: Arc<Mutex<Vec<PathBuf>>>,
    pub output_dir: Mutex<PathBuf>,
    pub motion_child: Arc<Mutex<Option<Child>>>,
    pub motion_active: Arc<AtomicBool>,
    pub preview_child: Arc<Mutex<Option<std::process::Child>>>,
    pub preview_path: Arc<Mutex<Option<PathBuf>>>,
    pub camera_preview_active: Arc<AtomicBool>,
    pub motion_last_detection: Arc<Mutex<Option<SystemTime>>>,
    pub finalizing_count: AtomicUsize,
    // Stealth mode — runtime toggle, not persisted
    pub stealth_mode: AtomicBool,
    // Time-lapse state
    pub timelapse_active: Arc<AtomicBool>,
    pub timelapse_last_capture: Arc<Mutex<Option<String>>>,
    // Recording start time for duration tracking
    pub recording_start_time: Arc<Mutex<Option<SystemTime>>>,
    pub screen_recording_start_time: Arc<Mutex<Option<SystemTime>>>,
    // Voice recognition
    pub voice_active: Arc<AtomicBool>,
    pub voice_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    pub voice_last_command: Arc<Mutex<Option<String>>>,
    pub voice_last_command_time: Arc<Mutex<Option<String>>>,
    // Audio level monitoring
    pub audio_level_active: Arc<AtomicBool>,
    pub audio_level_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Drop for AppState {
    fn drop(&mut self) {
        let settings = self.settings.lock().clone();
        self.motion_active.store(false, Ordering::SeqCst);
        self.timelapse_active.store(false, Ordering::SeqCst);
        let motion_child = self.motion_child.lock().take();
        if let Some(mut child) = motion_child {
            graceful_stop_child(&mut child);
        }

        let webcam_path = self.recording_output_path.lock().take();
        let screen_path = self.screen_recording_output_path.lock().take();
        let multi_paths: Vec<_> = self.multi_recording_output_paths.lock().drain(..).collect();

        let webcam_child = self.recording_child.lock().take();
        let screen_child = self.screen_recording_child.lock().take();
        let multi_children: Vec<_> = self.multi_recording_children.lock().drain(..).collect();

        if let Some(mut child) = webcam_child {
            graceful_stop_child(&mut child);
        }
        if let Some(mut child) = screen_child {
            graceful_stop_child(&mut child);
        }
        for mut child in multi_children {
            graceful_stop_child(&mut child);
        }

        if let Some(path) = webcam_path {
            let _ = post_process_capture_inner(&settings, &path);
        }
        if let Some(path) = screen_path {
            let _ = post_process_capture_inner(&settings, &path);
        }
        for path in multi_paths {
            let _ = post_process_capture_inner(&settings, &path);
        }

        let preview_child = self.preview_child.lock().take();
        if let Some(mut child) = preview_child {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Stop voice monitoring
        self.voice_active.store(false, Ordering::SeqCst);
        let handle = self.voice_thread.lock().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

#[derive(Clone, Copy)]
enum HotkeyAction {
    StartWebcam,
    StopWebcam,
    StartScreen,
    StopScreen,
    CaptureCurrent,
    CaptureAllMerged,
    ToggleMotionMode,
    CycleCameraLeft,
    CycleCameraRight,
    CaptureRegionSelector,
    ToggleStealth,
    ToggleTimelapse,
    KillDefeye,
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("defEYE")
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            get_status,
            update_hud_status,
            start_webcam_recording,
            stop_webcam_recording,
            start_screen_recording,
            stop_screen_recording,
            stop_any_recording,
            capture_current_screen,
            capture_all_screens_merged,
            list_recent_captures,
            get_capture_stats,
            list_cameras,
            list_monitors,
            list_audio_devices,
            open_path,
            open_url,
            reveal_path,
            delete_capture,
            delete_timelapse_session,
            create_timelapse_gif,
            run_defeye_analysis,
            test_ollama,
            list_ollama_models,
            toggle_motion_mode,
            get_motion_status,
            update_motion_settings,
            analyze_last_motion_clip,
            verify_integrity,
            cycle_camera,
            switch_screen_monitor,
            start_camera_preview,
            stop_camera_preview,
            start_region_selector,
            close_region_selector,
            set_region_from_selector,
            toggle_stealth_mode,
            get_disk_info,
            set_capture_note,
            get_capture_note,
            extract_snapshot,
            start_timelapse,
            stop_timelapse,
            get_timelapse_status,
            get_recording_duration,
            exit_app,
            set_system_tray_enabled,
            update_hotkey,
            reset_hotkeys,
            toggle_voice_control,
            get_voice_status,
            list_audio_input_devices,
            start_audio_level_monitor,
            stop_audio_level_monitor
        ])
        .setup(|app| {
            // Set the Vosk resource directory so the dynamic loader can find
            // libvosk.dll and its dependencies when installed via MSI/NSIS.
            if let Ok(res_dir) = app.path().resource_dir() {
                vosk_dynamic::set_resource_dir(res_dir);
            }

            let settings = load_or_create_settings(app.handle())?;
            ensure_dir(&settings.output_dir)?;

            app.manage(AppState {
                output_dir: Mutex::new(settings.output_dir.clone()),
                settings: Mutex::new(settings.clone()),
                is_recording: AtomicBool::new(false),
                recording_kind: Mutex::new("idle".to_string()),
                recording_child: Arc::new(Mutex::new(None)),
                screen_recording_child: Arc::new(Mutex::new(None)),
                multi_recording_children: Arc::new(Mutex::new(Vec::new())),
                active_camera_index: AtomicUsize::new(0),
                recording_output_path: Arc::new(Mutex::new(None)),
                screen_recording_output_path: Arc::new(Mutex::new(None)),
                multi_recording_output_paths: Arc::new(Mutex::new(Vec::new())),
                motion_child: Arc::new(Mutex::new(None)),
                motion_active: Arc::new(AtomicBool::new(false)),
                preview_child: Arc::new(Mutex::new(None)),
                preview_path: Arc::new(Mutex::new(None)),
                camera_preview_active: Arc::new(AtomicBool::new(false)),
                motion_last_detection: Arc::new(Mutex::new(None)),
                finalizing_count: AtomicUsize::new(0),
                stealth_mode: AtomicBool::new(false),
                timelapse_active: Arc::new(AtomicBool::new(false)),
                timelapse_last_capture: Arc::new(Mutex::new(None)),
                recording_start_time: Arc::new(Mutex::new(None)),
                screen_recording_start_time: Arc::new(Mutex::new(None)),
                voice_active: Arc::new(AtomicBool::new(false)),
                voice_thread: Arc::new(Mutex::new(None)),
                voice_last_command: Arc::new(Mutex::new(None)),
                voice_last_command_time: Arc::new(Mutex::new(None)),
                audio_level_active: Arc::new(AtomicBool::new(false)),
                audio_level_thread: Arc::new(Mutex::new(None)),
            });

            build_hidden_main_window(app.handle())?;
            build_hud_window(app.handle(), settings.hud_corner, settings.hud_minimal)?;
            build_region_selector_window(app.handle())?;
            register_global_shortcuts(app.handle(), &settings)?;

            if settings.system_tray_enabled {
                build_system_tray(app.handle())?;
            }

            match app.autolaunch().enable() {
                Ok(()) => eprintln!("[defEYE] Autolaunch enabled successfully"),
                Err(e) => eprintln!("[defEYE] Failed to enable autolaunch: {e}"),
            }
            emit_status(app.handle(), app.state::<AppState>().inner());

            if settings.motion_mode_enabled {
                start_motion_detection_inner(app.handle().clone(), app.state::<AppState>().inner())?;
            }

            spawn_watchdog(app.handle().clone());
            spawn_disk_sentinel(app.handle().clone());

            if settings.voice_control_enabled && settings.voice_auto_start {
                if let Err(e) = start_voice_monitoring(app.handle().clone(), app.state::<AppState>().inner()) {
                    eprintln!("[defEYE] Failed to start voice monitoring: {e}");
                }
            } else if settings.voice_control_enabled && !settings.voice_auto_start {
                // Voice control was enabled in a previous session but auto-start is off.
                // Reset the flag so the first click correctly starts monitoring.
                let mut s = app.state::<AppState>().settings.lock().clone();
                s.voice_control_enabled = false;
                let _ = save_settings(app.handle(), &s);
                *app.state::<AppState>().settings.lock() = s;
            }

            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("failed to run defEYE: {error}");
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> std::result::Result<Settings, String> {
    Ok(state.settings.lock().clone())
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> std::result::Result<(), String> {
    sanitize_settings(&mut settings).map_err(to_user_error)?;
    ensure_dir(&settings.output_dir).map_err(to_user_error)?;
    save_settings(&app, &settings).map_err(to_user_error)?;

    *state.output_dir.lock() = settings.output_dir.clone();
    let hud_corner = settings.hud_corner;
    let hud_minimal = settings.hud_minimal;
    let motion_was_enabled = state.settings.lock().motion_mode_enabled;
    let motion_now_enabled = settings.motion_mode_enabled;
    let old_hotkeys = state.settings.lock().hotkeys.clone();
    let new_hotkeys = settings.hotkeys.clone();
    let voice_now_enabled = settings.voice_control_enabled;

    // Capture old voice settings before updating state
    let old_voice = {
        let s = state.settings.lock();
        (
            s.voice_control_enabled,
            s.voice_confidence_threshold,
            s.voice_wake_word.clone(),
            s.voice_audio_device.clone(),
            s.voice_model_path.clone(),
            s.voice_commands.clone(),
        )
    };
    let new_voice = (
        settings.voice_control_enabled,
        settings.voice_confidence_threshold,
        settings.voice_wake_word.clone(),
        settings.voice_audio_device.clone(),
        settings.voice_model_path.clone(),
        settings.voice_commands.clone(),
    );

    *state.settings.lock() = settings;
    position_hud_window(&app, hud_corner, hud_minimal).map_err(to_user_error)?;

    if old_hotkeys != new_hotkeys {
        for s in &all_hotkey_shortcuts_from(&old_hotkeys) {
            let _ = app.global_shortcut().unregister(s.as_str());
        }
        let fresh = state.settings.lock().clone();
        register_global_shortcuts(&app, &fresh).map_err(to_user_error)?;
    }

    let _ = app.emit("settings-updated", state.settings.lock().clone());
    emit_status(&app, state.inner());

    if motion_was_enabled != motion_now_enabled {
        if motion_now_enabled {
            start_motion_detection_inner(app.clone(), state.inner()).map_err(to_user_error)?;
        } else {
            stop_motion_detection_inner(state.inner());
            emit_motion_status(&app, state.inner());
        }
    }

    // Restart voice recognition if voice-related settings changed while active
    let voice_active = state.voice_active.load(Ordering::SeqCst);
    if voice_active && voice_now_enabled && old_voice != new_voice {
        restart_voice_monitoring_async(app.clone());
    }
    Ok(())
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> std::result::Result<StatusPayload, String> {
    Ok(status_payload(state.inner()))
}

#[tauri::command]
fn update_hud_status(
    app: AppHandle,
    state: State<'_, AppState>,
    recording_kind: String,
) -> std::result::Result<(), String> {
    let normalized = if state.is_recording.load(Ordering::SeqCst) {
        sanitize_recording_kind(&recording_kind)
    } else {
        "idle".to_string()
    };
    *state.recording_kind.lock() = normalized;
    emit_status(&app, state.inner());
    Ok(())
}

#[tauri::command]
fn start_webcam_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    start_recording_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn stop_webcam_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    stop_webcam_recording_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn start_screen_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    start_screen_recording_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn stop_screen_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    stop_screen_recording_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn stop_any_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    stop_all_recording_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn capture_current_screen(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    capture_current_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn capture_all_screens_merged(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    capture_all_merged_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn list_recent_captures(
    state: State<'_, AppState>,
) -> std::result::Result<Vec<CaptureInfo>, String> {
    list_recent_captures_inner(state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn get_capture_stats(
    state: State<'_, AppState>,
) -> std::result::Result<CaptureStats, String> {
    let output_dir = state.output_dir.lock().clone();
    get_capture_stats_inner(&output_dir).map_err(to_user_error)
}

#[tauri::command]
fn list_cameras() -> std::result::Result<Vec<String>, String> {
    list_cameras_inner().map_err(to_user_error)
}

#[tauri::command]
fn list_monitors() -> std::result::Result<Vec<MonitorInfo>, String> {
    list_monitors_inner().map_err(to_user_error)
}

#[tauri::command]
fn list_audio_devices() -> std::result::Result<Vec<AudioDevice>, String> {
    list_audio_devices_inner().map_err(to_user_error)
}

#[tauri::command]
fn verify_integrity(path: String) -> std::result::Result<IntegrityResult, String> {
    verify_integrity_inner(&path).map_err(to_user_error)
}

#[tauri::command]
fn cycle_camera(
    app: AppHandle,
    state: State<'_, AppState>,
    direction: i32,
) -> std::result::Result<String, String> {
    cycle_camera_inner(&app, state.inner(), direction).map_err(to_user_error)
}

#[tauri::command]
fn switch_screen_monitor(
    app: AppHandle,
    state: State<'_, AppState>,
    monitor_id: String,
) -> std::result::Result<String, String> {
    switch_screen_monitor_inner(&app, state.inner(), &monitor_id).map_err(to_user_error)
}

#[tauri::command]
fn start_camera_preview(
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    if state.camera_preview_active.load(Ordering::SeqCst) {
        return Err("Camera preview is already active".to_string());
    }

    let settings = state.settings.lock().clone();
    let video_device = effective_camera_device(&settings);
    if video_device.is_empty() {
        return Err("No camera device selected".to_string());
    }

    let dshow_input = build_webcam_dshow_input(&settings, &video_device);

    let preview_dir = std::env::temp_dir().join("defeye_preview");
    let _ = fs::create_dir_all(&preview_dir);
    let preview_file = preview_dir.join("preview.jpg");

    eprintln!("[defEYE] preview ffmpeg input: {dshow_input}");

    state.camera_preview_active.store(true, Ordering::SeqCst);
    *state.preview_path.lock() = Some(preview_file.clone());

    let active = state.camera_preview_active.clone();
    let input = dshow_input.clone();
    let out_path = preview_file.clone();

    thread::spawn(move || {
        while active.load(Ordering::SeqCst) {
            let mut cmd = ffmpeg_command();
            cmd.arg("-y");
            cmd.args(["-f", "dshow", "-rtbufsize", "50M", "-i", &input]);
            cmd.args(["-vf", "scale=640:-1"]);
            cmd.args(["-frames:v", "1"]);
            cmd.arg(&out_path);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            match cmd.spawn() {
                Ok(mut child) => {
                    let deadline = std::time::Instant::now() + Duration::from_secs(3);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break,
                            Ok(None) if std::time::Instant::now() < deadline => {
                                std::thread::sleep(Duration::from_millis(50));
                            }
                            _ => {
                                let _ = child.kill();
                                let _ = child.wait();
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[defEYE] preview frame capture failed: {e}");
                    break;
                }
            }

            for _ in 0..10 {
                if !active.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = fs::remove_file(&out_path);
    });

    Ok(preview_file.to_string_lossy().to_string())
}

#[tauri::command]
fn stop_camera_preview(state: State<'_, AppState>) -> std::result::Result<(), String> {
    state.camera_preview_active.store(false, Ordering::SeqCst);
    let path = state.preview_path.lock().take();
    if let Some(p) = path {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

#[tauri::command]
fn start_region_selector(app: AppHandle) -> std::result::Result<(), String> {
    let Some(window) = app.get_webview_window(REGION_SELECTOR_LABEL) else {
        return Err("Region selector window not found".to_string());
    };

    let _ = window.show();
    let _ = window.set_fullscreen(true);
    let _ = window.set_focus();
    Ok(())
}

#[tauri::command]
fn close_region_selector(app: AppHandle) -> std::result::Result<(), String> {
    if let Some(window) = app.get_webview_window(REGION_SELECTOR_LABEL) {
        let _ = window.set_fullscreen(false);
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command]
fn set_region_from_selector(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> std::result::Result<(), String> {
    let _ = app.emit("region-selected", serde_json::json!({ "x": x, "y": y, "width": width, "height": height }));
    if let Some(window) = app.get_webview_window(REGION_SELECTOR_LABEL) {
        let _ = window.set_fullscreen(false);
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command]
#[allow(deprecated)]
fn open_url(app: AppHandle, url: String) -> std::result::Result<(), String> {
    app.shell()
        .open(url, None)
        .map_err(|error| format!("Failed to open URL: {error}"))
}

#[tauri::command]
#[allow(deprecated)]
fn open_path(app: AppHandle, path: String) -> std::result::Result<(), String> {
    let target = if path == "defeye://config-folder" {
        app.path()
            .app_local_data_dir()
            .map_err(|error| format!("Failed to locate config folder: {error}"))?
    } else {
        sanitize_existing_path(PathBuf::from(path)).map_err(to_user_error)?
    };

    app.shell()
        .open(target.to_string_lossy().to_string(), None)
        .map_err(|error| format!("Failed to open path: {error}"))
}

#[tauri::command]
fn reveal_path(path: String) -> std::result::Result<(), String> {
    let target = sanitize_existing_path(PathBuf::from(path)).map_err(to_user_error)?;
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(|error| format!("Failed to reveal path in Explorer: {error}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let parent = target
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Failed to locate parent directory.".to_string())?;
        tauri_plugin_shell::open::open(None, parent.to_string_lossy(), None)
            .map_err(|error| format!("Failed to reveal path: {error}"))
    }
}

#[tauri::command]
fn delete_capture(state: State<'_, AppState>, path: String) -> std::result::Result<(), String> {
    let target = sanitize_existing_path(PathBuf::from(path)).map_err(to_user_error)?;
    let output_dir = state.output_dir.lock().clone();
    ensure_inside_dir(&target, &output_dir).map_err(to_user_error)?;
    delete_capture_files(&target, &output_dir).map_err(to_user_error)
}

#[tauri::command]
fn delete_timelapse_session(
    state: State<'_, AppState>,
    session: String,
) -> std::result::Result<(), String> {
    let output_dir = state.output_dir.lock().clone();
    let session_dir = output_dir.join("timelapse").join(&session);
    if !session_dir.exists() {
        return Err(format!("Session directory not found: {session}"));
    }
    ensure_inside_dir(&session_dir, &output_dir).map_err(to_user_error)?;
    let entries = fs::read_dir(&session_dir).with_context(|| format!("Failed to read session dir {}", session_dir.display())).map_err(to_user_error)?;
    let mut count = 0u32;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let _ = delete_capture_files(&path, &output_dir);
            count += 1;
        }
    }
    let _ = fs::remove_dir(&session_dir);
    eprintln!("[defEYE] Deleted timelapse session {session}: {count} files");
    Ok(())
}

#[tauri::command]
fn create_timelapse_gif(
    state: State<'_, AppState>,
    session: String,
) -> std::result::Result<String, String> {
    let output_dir = state.output_dir.lock().clone();
    let session_dir = output_dir.join("timelapse").join(&session);
    if !session_dir.exists() {
        return Err(format!("Session directory not found: {session}"));
    }
    ensure_inside_dir(&session_dir, &output_dir).map_err(to_user_error)?;

    let gif_path = session_dir.join("timelapse.gif");

    // Collect and sort PNG files to build a proper ffmpeg concat input
    let mut png_files: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&session_dir).map_err(|e| format!("Failed to read session dir: {e}"))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("png") {
            png_files.push(path);
        }
    }
    png_files.sort();

    if png_files.is_empty() {
        return Err("No PNG frames found in session directory".to_string());
    }

    // Write a concat file for ffmpeg with duration directives.
    // PNG image files have 0 stream duration, so without explicit `duration`
    // directives the concat demuxer produces a 0-length video and the GIF fails.
    // Use a fixed 0.2s per frame (5 fps) for smooth GIF playback regardless of
    // the original timelapse capture interval.
    let frame_duration = 0.2f64;
    let concat_file = session_dir.join("concat_list.txt");
    let escaped: Vec<String> = png_files.iter()
        .map(|p| p.to_string_lossy().replace('\\', "/").replace('\'', "'\\''"))
        .collect();
    let mut concat_content = String::new();
    for path in &escaped {
        concat_content.push_str(&format!("file '{path}'\nduration {frame_duration}\n"));
    }
    // Repeat the last file without a duration so the concat demuxer gives the
    // final frame its full duration (otherwise it defaults to 0).
    if let Some(last) = escaped.last() {
        concat_content.push_str(&format!("file '{last}'\n"));
    }
    fs::write(&concat_file, &concat_content).map_err(|e| format!("Failed to write concat file: {e}"))?;

    let output = ffmpeg_command()
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(concat_file.to_string_lossy().as_ref())
        .arg("-vf")
        .arg(format!("fps=5,scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse"))
        .args(["-loop", "0"])
        .arg(gif_path.to_string_lossy().as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run ffmpeg for GIF: {e}"))?;

    let _ = fs::remove_file(&concat_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg failed to create GIF: {}", stderr.lines().last().unwrap_or("unknown error")));
    }

    if !gif_path.exists() {
        return Err("GIF file was not created".to_string());
    }

    eprintln!("[defEYE] Created GIF: {}", gif_path.display());
    Ok(gif_path.to_string_lossy().to_string())
}

#[tauri::command]
fn run_defeye_analysis(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: Option<String>,
    prompt: String,
) -> std::result::Result<AnalysisResult, String> {
    run_defeye_analysis_inner(&app, state.inner(), file_path, prompt)
}

fn run_defeye_analysis_inner(
    app: &AppHandle,
    state: &AppState,
    file_path: Option<String>,
    prompt: String,
) -> std::result::Result<AnalysisResult, String> {
    let settings = state.settings.lock().clone();
    let target = match file_path {
        Some(path) if !path.trim().is_empty() => {
            sanitize_existing_path(PathBuf::from(path)).map_err(to_user_error)?
        }
        _ => newest_capture_path(state).map_err(to_user_error)?,
    };

    let output_dir = state.output_dir.lock().clone();
    ensure_inside_dir(&target, &output_dir).map_err(to_user_error)?;
    let metadata = fs::metadata(&target)
        .with_context(|| format!("Failed to read {}", target.display()))
        .map_err(to_user_error)?;
    let dimensions = image::image_dimensions(&target)
        .ok()
        .map(|(width, height)| format!("{width}x{height}"));
    let monitors = if target
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("allmerged"))
    {
        run_with_com(|| Ok(Monitor::all().ok().map(|items| items.len()))).unwrap_or(None)
    } else {
        None
    };

    let captured = metadata
        .created()
        .or_else(|_| metadata.modified())
        .map(format_system_time)
        .unwrap_or_else(|_| "unknown".to_string());
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();

    let analysis_metadata = AnalysisMetadata {
        file: filename.clone(),
        captured: captured.clone(),
        monitors,
        resolution: dimensions.clone(),
        size: metadata.len(),
        prompt: prompt.clone(),
    };

    // If Ollama is not enabled, return metadata-only fallback
    if !settings.ollama_enabled {
        let monitor_text = monitors
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let resolution_text = dimensions.unwrap_or_else(|| "unknown".to_string());
        let analysis_text = format!(
            "[defEYE Metadata-Only Mode]\nFile: {filename}\nCaptured: {captured}\nMonitors: {monitor_text} | Resolution: {resolution_text}\n\nAI analysis unavailable — Ollama not enabled.\nEnable Ollama in AI Settings to activate vision analysis.\n\nPrompt: {prompt}"
        );
        return Ok(AnalysisResult {
            metadata: analysis_metadata,
            analysis_text,
            confidence: 0.0,
            tags: vec![],
            observations: vec![],
            raw_response: String::new(),
        });
    }

    // Prepare image for Ollama (videos need a keyframe extracted first)
    let is_video = target
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mp4"))
        .unwrap_or(false);
    let image_b64 = if is_video {
        extract_video_keyframe_base64(&target).map_err(to_user_error)?
    } else {
        prepare_image_for_ollama(&target).map_err(to_user_error)?
    };
    let model = if settings.ollama_model.trim().is_empty() {
        "qwen2.5vl:7b"
    } else {
        settings.ollama_model.trim()
    };

    let result = call_ollama(
        &settings.ollama_endpoint,
        model,
        &settings.ollama_system_prompt,
        &prompt,
        Some(&image_b64),
        settings.ollama_temperature,
        settings.ollama_max_tokens,
    );

    let (analysis_text, confidence, tags, observations, raw_response) = match result {
        Ok(raw) => {
            let parsed = parse_ollama_response(&raw);
            (
                parsed.summary,
                parsed.confidence,
                parsed.tags,
                parsed.observations,
                raw,
            )
        }
        Err(e) => {
            let err_msg = format!("AI analysis failed: {e}");
            eprintln!("[defEYE] {err_msg}");
            (
                format!(
                    "[defEYE Analysis Error]\nFile: {filename}\n\n{err_msg}\n\nCheck that Ollama is running ('ollama serve') and the model '{model}' is pulled ('ollama pull {model}')."
                ),
                0.0,
                vec![],
                vec![],
                String::new(),
            )
        }
    };

    let result = AnalysisResult {
        metadata: analysis_metadata,
        analysis_text,
        confidence,
        tags,
        observations,
        raw_response,
    };

    // Save sidecar JSON
    let _ = save_analysis_sidecar(&target, &result);

    // Emit analysis-complete event
    let _ = app.emit("analysis-complete", &result);

    Ok(result)
}

#[tauri::command]
fn toggle_motion_mode(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<bool, String> {
    let mut settings = state.settings.lock().clone();
    settings.motion_mode_enabled = !settings.motion_mode_enabled;
    let now_enabled = settings.motion_mode_enabled;

    save_settings(&app, &settings).map_err(to_user_error)?;
    *state.settings.lock() = settings;

    if now_enabled {
        start_motion_detection_inner(app.clone(), state.inner()).map_err(to_user_error)?;
    } else {
        stop_motion_detection_inner(state.inner());
    }
    emit_motion_status(&app, state.inner());
    Ok(now_enabled)
}

#[tauri::command]
fn get_motion_status(state: State<'_, AppState>) -> std::result::Result<MotionStatusPayload, String> {
    let settings = state.settings.lock();
    let motion_active = state.motion_active.load(Ordering::SeqCst);
    let last_detection = state
        .motion_last_detection
        .lock()
        .map(|time| {
            let datetime: DateTime<Local> = DateTime::<Local>::from(time);
            datetime.to_rfc3339()
        });
    Ok(MotionStatusPayload {
        motion_mode_enabled: settings.motion_mode_enabled,
        motion_active,
        last_detection,
    })
}

#[tauri::command]
fn update_motion_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> std::result::Result<(), String> {
    sanitize_settings(&mut settings).map_err(to_user_error)?;
    save_settings(&app, &settings).map_err(to_user_error)?;

    // Lock settings once, capture previous values, then overwrite
    let (prev_enabled, prev_sensitivity, prev_cooldown, prev_triggers_screen, prev_auto_record) = {
        let current = state.settings.lock();
        (
            current.motion_mode_enabled,
            current.motion_sensitivity,
            current.motion_cooldown_seconds,
            current.motion_triggers_screen,
            current.auto_record_on_motion,
        )
    };

    let now_enabled = settings.motion_mode_enabled;
    let sensitivity_changed = settings.motion_sensitivity != prev_sensitivity;
    let cooldown_changed = settings.motion_cooldown_seconds != prev_cooldown;
    let triggers_changed = settings.motion_triggers_screen != prev_triggers_screen;
    let auto_record_changed = settings.auto_record_on_motion != prev_auto_record;
    let enabled_changed = now_enabled != prev_enabled;
    *state.settings.lock() = settings;

    if enabled_changed {
        if now_enabled {
            start_motion_detection_inner(app.clone(), state.inner()).map_err(to_user_error)?;
        } else {
            stop_motion_detection_inner(state.inner());
        }
    } else if now_enabled
        && (sensitivity_changed || cooldown_changed || triggers_changed || auto_record_changed)
    {
        restart_motion_detection_async(app.clone());
    }
    emit_motion_status(&app, state.inner());
    Ok(())
}

#[tauri::command]
fn analyze_last_motion_clip(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
) -> std::result::Result<AnalysisResult, String> {
    let settings = state.settings.lock().clone();
    let output_dir = state.output_dir.lock().clone();
    ensure_dir(&output_dir).map_err(to_user_error)?;

    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(&output_dir)
        .with_context(|| format!("Failed to read {}", output_dir.display()))
        .map_err(to_user_error)?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !filename.starts_with("defEYE_webcam_") || !filename.ends_with(".mp4") {
            continue;
        }
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let created = metadata
            .created()
            .or_else(|_| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.as_ref().map_or(true, |(t, _)| &created > t) {
            newest = Some((created, path));
        }
    }

    let target = newest
        .map(|(_, p)| p)
        .ok_or_else(|| "No motion webcam clips found.".to_string())?;

    let metadata = fs::metadata(&target)
        .with_context(|| format!("Failed to read {}", target.display()))
        .map_err(to_user_error)?;
    let dimensions = image::image_dimensions(&target)
        .ok()
        .map(|(w, h)| format!("{w}x{h}"));
    let captured = metadata
        .created()
        .or_else(|_| metadata.modified())
        .map(format_system_time)
        .unwrap_or_else(|_| "unknown".to_string());
    let filename = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let analysis_metadata = AnalysisMetadata {
        file: filename.clone(),
        captured: captured.clone(),
        monitors: None,
        resolution: dimensions.clone(),
        size: metadata.len(),
        prompt: prompt.clone(),
    };

    if !settings.ollama_enabled {
        let resolution_text = dimensions.unwrap_or_else(|| "unknown".to_string());
        let analysis_text = format!(
            "[defEYE Metadata-Only Mode]\nFile: {filename}\nCaptured: {captured}\nResolution: {resolution_text}\n\nAI analysis unavailable — Ollama not enabled.\nEnable Ollama in AI Settings to activate vision analysis.\n\nPrompt: {prompt}"
        );
        return Ok(AnalysisResult {
            metadata: analysis_metadata,
            analysis_text,
            confidence: 0.0,
            tags: vec![],
            observations: vec![],
            raw_response: String::new(),
        });
    }

    // Extract keyframe from MP4 for analysis
    let image_b64 = match extract_video_keyframe_base64(&target) {
        Ok(b64) => b64,
        Err(_) => {
            return Ok(AnalysisResult {
                metadata: analysis_metadata,
                analysis_text: format!("[defEYE Analysis Error]\nFile: {filename}\n\nFailed to extract keyframe from video for analysis."),
                confidence: 0.0,
                tags: vec![],
                observations: vec![],
                raw_response: String::new(),
            });
        }
    };

    let model = if settings.ollama_model.trim().is_empty() {
        "qwen2.5vl:7b"
    } else {
        settings.ollama_model.trim()
    };

    let result = call_ollama(
        &settings.ollama_endpoint,
        model,
        &settings.ollama_system_prompt,
        &prompt,
        Some(&image_b64),
        settings.ollama_temperature,
        settings.ollama_max_tokens,
    );

    let (analysis_text, confidence, tags, observations, raw_response) = match result {
        Ok(raw) => {
            let parsed = parse_ollama_response(&raw);
            (parsed.summary, parsed.confidence, parsed.tags, parsed.observations, raw)
        }
        Err(e) => {
            let err_msg = format!("AI analysis failed: {e}");
            eprintln!("[defEYE] {err_msg}");
            (
                format!("[defEYE Analysis Error]\nFile: {filename}\n\n{err_msg}\n\nCheck that Ollama is running ('ollama serve') and the model '{model}' is pulled ('ollama pull {model}')."),
                0.0, vec![], vec![], String::new(),
            )
        }
    };

    let result = AnalysisResult {
        metadata: analysis_metadata,
        analysis_text,
        confidence,
        tags,
        observations,
        raw_response,
    };

    let _ = save_analysis_sidecar(&target, &result);
    let _ = app.emit("analysis-complete", &result);

    Ok(result)
}

// ---------------------------------------------------------------------------
// Ollama AI integration helpers
// ---------------------------------------------------------------------------

fn prepare_image_for_ollama(path: &Path) -> Result<String> {
    let img = image::open(path)
        .with_context(|| format!("Failed to open image: {}", path.display()))?;
    // Resize to max 1024px width for efficiency
    let resized = if img.width() > 1024 {
        let ratio = 1024.0 / img.width() as f32;
        let target_h = ((img.height() as f32) * ratio).round() as u32;
        img.resize(1024, target_h.max(1), image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    let format = if path.extension().and_then(|e| e.to_str()) == Some("jpg") || path.extension().and_then(|e| e.to_str()) == Some("jpeg") {
        ImageFormat::Jpeg
    } else {
        ImageFormat::Png
    };
    resized.write_to(&mut buf, format)
        .with_context(|| "Failed to encode image for Ollama")?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

/// Extract a single keyframe from a video file, encode it, and return the base64 payload.
fn extract_video_keyframe_base64(video_path: &Path) -> Result<String> {
    let keyframe_path = std::env::temp_dir().join(format!("defeye_keyframe_{}.jpg", timestamp()));
    let keyframe_arg = keyframe_path.to_string_lossy().to_string();
    let file_arg = video_path.to_string_lossy().to_string();

    let extract = |offset: Option<&str>| -> std::io::Result<std::process::ExitStatus> {
        let mut cmd = ffmpeg_command();
        cmd.args(["-y"]);
        if let Some(off) = offset {
            cmd.args(["-ss", off]);
        }
        cmd.args([
            "-i",
            &file_arg,
            "-frames:v",
            "1",
            "-vf",
            "scale=1024:-1",
            "-f",
            "image2",
            &keyframe_arg,
        ]);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };

    // Try 1 second in to avoid blank first frames; fall back to the first frame.
    let _ = extract(Some("00:00:01"));
    if !keyframe_path.exists() {
        let _ = extract(None);
    }

    let result = if keyframe_path.exists() {
        prepare_image_for_ollama(&keyframe_path).context("Failed to encode extracted keyframe")
    } else {
        bail!("Failed to extract keyframe from video for analysis.")
    };

    let _ = fs::remove_file(&keyframe_path);
    result
}

struct ParsedOllamaResponse {
    summary: String,
    confidence: f32,
    tags: Vec<String>,
    observations: Vec<String>,
}

fn call_ollama(
    endpoint: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    image_b64: Option<&str>,
    temperature: f32,
    max_tokens: u32,
) -> Result<String> {
    let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| anyhow!("Failed to create HTTP client: {e}"))?;

    let mut user_msg = serde_json::json!({
        "role": "user",
        "content": user_prompt,
    });
    if let Some(b64) = image_b64 {
        user_msg["images"] = serde_json::json!([b64]);
    }
    let messages = serde_json::json!([
        { "role": "system", "content": system_prompt },
        user_msg,
    ]);

    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "options": {
            "temperature": temperature,
            "num_predict": max_tokens,
        }
    });

    let resp = client.post(&url)
        .json(&payload)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                anyhow!("Cannot connect to Ollama at {url}. Is 'ollama serve' running?")
            } else if e.is_timeout() {
                anyhow!("Ollama request timed out. Try a smaller model or lower max_tokens.")
            } else {
                anyhow!("Ollama request failed: {e}")
            }
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        if status.as_u16() == 404 {
            bail!("Model '{model}' not found in Ollama. Pull it with: ollama pull {model}");
        }
        if body.contains("unknown model architecture") {
            // Auto-fallback: if llama3.2-vision fails on newer Ollama, retry with qwen2.5vl:7b
            if model == "llama3.2-vision:11b" {
                eprintln!("[defEYE] llama3.2-vision:11b not supported by this Ollama version, falling back to qwen2.5vl:7b");
                return call_ollama(endpoint, "qwen2.5vl:7b", system_prompt, user_prompt, image_b64, temperature, max_tokens);
            }
            bail!(
                "Ollama cannot load model '{model}' — its architecture is not supported by your Ollama version.\n\
                 This is a known issue with llama3.2-vision on Ollama 0.30.x (see github.com/ollama/ollama/issues/16490).\n\n\
                 Fix: Switch to a supported vision model (e.g., 'ollama pull qwen2.5vl:7b') and update the model in AI Settings."
            );
        }
        bail!("Ollama returned HTTP {status}: {body}");
    }

    let resp_json: serde_json::Value = resp.json()
        .map_err(|e| anyhow!("Failed to parse Ollama response: {e}"))?;

    let content = resp_json
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    Ok(content.to_string())
}

fn parse_ollama_response(raw: &str) -> ParsedOllamaResponse {
    // Try to extract JSON from the response
    let trimmed = raw.trim();

    // Find JSON object in the response
    let json_start = trimmed.find('{');
    let json_end = trimmed.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &trimmed[start..=end];
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            let summary = parsed.get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or(raw)
                .to_string();
            let confidence = parsed.get("confidence")
                .and_then(|v| v.as_f64())
                .map(|f| f as f32)
                .unwrap_or(0.5);
            let tags = parsed.get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let observations = parsed.get("key_observations")
                .or_else(|| parsed.get("observations"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|o| o.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            return ParsedOllamaResponse { summary, confidence, tags, observations };
        }
    }

    // Fallback: use raw text as summary
    ParsedOllamaResponse {
        summary: raw.to_string(),
        confidence: 0.5,
        tags: vec![],
        observations: vec![],
    }
}

fn save_analysis_sidecar(file_path: &Path, result: &AnalysisResult) -> Result<()> {
    let sidecar_path = file_path.with_extension("analysis.json");
    let json = serde_json::to_string_pretty(result)
        .with_context(|| "Failed to serialize analysis result")?;
    fs::write(&sidecar_path, json)
        .with_context(|| format!("Failed to write analysis sidecar: {}", sidecar_path.display()))?;
    Ok(())
}

#[tauri::command]
fn test_ollama(endpoint: String) -> std::result::Result<String, String> {
    let url = format!("{}/api/version", endpoint.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let resp = client.get(&url).send().map_err(|e| {
        if e.is_connect() {
            format!("Cannot connect to Ollama at {url}. Is 'ollama serve' running?")
        } else {
            format!("Connection failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(format!("Ollama returned HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().map_err(|e| format!("Failed to parse response: {e}"))?;
    let version = body.get("version").and_then(|v| v.as_str()).unwrap_or("unknown");
    Ok(version.to_string())
}

#[tauri::command]
fn list_ollama_models(endpoint: String) -> std::result::Result<Vec<String>, String> {
    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let resp = client.get(&url).send().map_err(|e| {
        if e.is_connect() {
            format!("Cannot connect to Ollama at {url}. Is 'ollama serve' running?")
        } else {
            format!("Connection failed: {e}")
        }
    })?;
    if !resp.status().is_success() {
        return Err(format!("Ollama returned HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().map_err(|e| format!("Failed to parse response: {e}"))?;
    let models = body.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

// ---------------------------------------------------------------------------
// Stealth Mode
// ---------------------------------------------------------------------------

fn toggle_stealth_mode_inner(app: &AppHandle) -> Result<bool> {
    let main = app.get_webview_window(MAIN_LABEL);
    let visible = main.as_ref().map(|w| w.is_visible().unwrap_or(false)).unwrap_or(false);

    if visible {
        if let Some(w) = &main { let _ = w.hide(); }
        if let Some(w) = app.get_webview_window(HUD_LABEL) { let _ = w.hide(); }
        eprintln!("[defEYE] Stealth: HIDDEN");
        Ok(true)
    } else {
        if let Some(w) = &main {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        let state = app.state::<AppState>();
        let settings = state.settings.lock();
        if let Some(w) = app.get_webview_window(HUD_LABEL) {
            if !matches!(settings.hud_corner, HudCorner::Hidden) {
                let _ = w.unminimize();
                let _ = w.show();
            }
        }
        drop(settings);
        eprintln!("[defEYE] Stealth: SHOWN");
        Ok(false)
    }
}

#[tauri::command]
fn toggle_stealth_mode(app: AppHandle) -> std::result::Result<bool, String> {
    toggle_stealth_mode_inner(&app).map_err(to_user_error)
}

// ---------------------------------------------------------------------------
// Disk Sentinel
// ---------------------------------------------------------------------------

/// Query free and total disk bytes for a directory on Windows.
/// Returns (free_bytes, total_bytes) or None if the API call fails.
#[cfg(target_os = "windows")]
fn query_disk_free_space(dir: &Path) -> Option<(u64, u64)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let mut free_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let wide: Vec<u16> = OsStr::new(dir)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let success = unsafe {
        windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            windows::core::PCWSTR(wide.as_ptr()),
            Some(&mut free_bytes),
            Some(&mut total_bytes),
            None,
        )
    }.is_ok();

    if success { Some((free_bytes, total_bytes)) } else { None }
}

#[tauri::command]
fn get_disk_info(state: State<'_, AppState>) -> std::result::Result<DiskInfoPayload, String> {
    let output_dir = state.output_dir.lock().clone();
    let threshold_mb = state.settings.lock().disk_threshold_mb;

    #[cfg(target_os = "windows")]
    {
        let (free_bytes, total_bytes) = query_disk_free_space(&output_dir)
            .ok_or_else(|| "Failed to query disk space".to_string())?;
        let free_mb = free_bytes / (1024 * 1024);
        let warning = threshold_mb > 0 && free_mb < threshold_mb as u64;
        return Ok(DiskInfoPayload {
            free_bytes,
            total_bytes,
            free_mb,
            threshold_mb,
            warning,
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(DiskInfoPayload {
            free_bytes: 0,
            total_bytes: 0,
            free_mb: 0,
            threshold_mb,
            warning: false,
        })
    }
}

fn check_disk_space(output_dir: &Path, threshold_mb: u32) -> bool {
    if threshold_mb == 0 {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        match query_disk_free_space(output_dir) {
            Some((free_bytes, _)) => free_bytes / (1024 * 1024) >= threshold_mb as u64,
            None => true,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

fn spawn_disk_sentinel(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(DISK_CHECK_INTERVAL);

            let state = app.state::<AppState>();
            let settings = state.settings.lock().clone();
            let threshold = settings.disk_threshold_mb;
            drop(settings);

            if threshold == 0 {
                continue;
            }

            let output_dir = state.output_dir.lock().clone();
            if !check_disk_space(&output_dir, threshold) {
                eprintln!("[defEYE] Disk Sentinel: disk space below {threshold}MB threshold, stopping all recordings");
                let _ = stop_all_recording_inner(&app, state.inner());
                let _ = app.emit("defeye-error", format!("Disk space below {threshold}MB — recordings stopped to prevent disk exhaustion."));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Capture Annotations
// ---------------------------------------------------------------------------

#[tauri::command]
fn set_capture_note(path: String, note: String) -> std::result::Result<(), String> {
    let file_path = sanitize_existing_path(PathBuf::from(&path)).map_err(to_user_error)?;
    let note_path = file_path.with_extension("note.json");
    let note_trimmed = note.trim();
    if note_trimmed.is_empty() {
        if note_path.exists() {
            let _ = fs::remove_file(&note_path);
        }
        return Ok(());
    }
    let sidecar = serde_json::json!({
        "file": file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "note": note_trimmed,
        "timestamp": timestamp(),
    });
    let json = serde_json::to_string_pretty(&sidecar).map_err(|e| to_user_error(anyhow::Error::from(e)))?;
    fs::write(&note_path, json).map_err(|e| to_user_error(anyhow::Error::from(e)))?;
    Ok(())
}

#[tauri::command]
fn get_capture_note(path: String) -> std::result::Result<CaptureNotePayload, String> {
    let file_path = sanitize_existing_path(PathBuf::from(&path)).map_err(to_user_error)?;
    let note_path = file_path.with_extension("note.json");
    if !note_path.exists() {
        return Ok(CaptureNotePayload {
            path: path.clone(),
            note: None,
        });
    }
    let text = fs::read_to_string(&note_path).map_err(|e| to_user_error(anyhow::Error::from(e)))?;
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|e| to_user_error(anyhow::Error::from(e)))?;
    let note = parsed.get("note").and_then(|v| v.as_str()).map(|s| s.to_string());
    Ok(CaptureNotePayload {
        path: path.clone(),
        note,
    })
}

// ---------------------------------------------------------------------------
// Snapshot Extractor
// ---------------------------------------------------------------------------

#[tauri::command]
fn extract_snapshot(
    state: State<'_, AppState>,
    video_path: String,
    timestamp_secs: f64,
) -> std::result::Result<String, String> {
    let file_path = sanitize_existing_path(PathBuf::from(&video_path)).map_err(to_user_error)?;
    let output_dir = state.output_dir.lock().clone();
    ensure_inside_dir(&file_path, &output_dir).map_err(to_user_error)?;

    let output = output_dir.join(format!(
        "defEYE_snapshot_{}.png",
        timestamp()
    ));

    let ss_arg = format!("{:.2}", timestamp_secs);
    let file_arg = file_path.to_string_lossy().to_string();
    let out_arg = output.to_string_lossy().to_string();

    let status = FfmpegCommandBuilder::new()
        .args(&["-ss", &ss_arg, "-i", &file_arg, "-frames:v", "1", "-vf", "scale=-1:-1", "-f", "image2", &out_arg])
        .stdio_silent()
        .run_silent()
        .map_err(|e| format!("Failed to extract snapshot: {e}"))?;

    if !status.success() {
        return Err("ffmpeg failed to extract snapshot at specified timestamp".to_string());
    }

    if !output.exists() {
        return Err("Snapshot file was not created".to_string());
    }

    let settings = state.settings.lock().clone();
    post_process_capture_inner(&settings, &output).map_err(to_user_error)?;
    Ok(output.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Time-Lapse Capture
// ---------------------------------------------------------------------------

#[tauri::command]
fn start_timelapse(app: AppHandle, state: State<'_, AppState>) -> std::result::Result<(), String> {
    start_timelapse_inner(app, state.inner()).map_err(to_user_error)
}

fn start_timelapse_inner(app: AppHandle, state: &AppState) -> Result<()> {
    if state.timelapse_active.load(Ordering::SeqCst) {
        bail!("Time-lapse is already active.");
    }
    let settings = state.settings.lock().clone();
    let interval = settings.timelapse_interval_seconds.max(TIMELAPSE_MIN_INTERVAL);
    let target = settings.timelapse_target;
    let output_dir = settings.output_dir.clone();
    let timelapse_dir = output_dir.join("timelapse");
    let session_folder = timelapse_dir.join(format!("session_{}", timestamp()));
    let settings_snapshot = settings.clone();

    ensure_dir(&output_dir)?;
    ensure_dir(&timelapse_dir)?;
    ensure_dir(&session_folder)?;
    state.timelapse_active.store(true, Ordering::SeqCst);

    let active = state.timelapse_active.clone();
    let app_handle = app.clone();
    let last_capture_ref = state.timelapse_last_capture.clone();

    let _ = app.emit("timelapse-started", timelapse_status_payload_from(
        true,
        interval,
        &target,
        Path::new(""),
    ));

    thread::spawn(move || {
        eprintln!("[defEYE] Time-lapse started: interval={interval}s, target={:?}", target);
        let mut frame_counter: u64 = 0;
        while active.load(Ordering::SeqCst) {
            let mut outputs: Vec<PathBuf> = Vec::new();
            frame_counter += 1;
            let frame_ts = format!("{}_{:06}", timestamp(), frame_counter);

            match target {
                TimeLapseTarget::Screen | TimeLapseTarget::Both => {
                    let output = session_folder.join(format!("defEYE_timelapse_screen_{}.png", frame_ts));
                    let output_for_closure = output.clone();
                    let settings_for_capture = settings_snapshot.clone();
                    let result = run_with_com(move || {
                        let monitor = selected_monitor(&settings_for_capture)?;
                        let image = monitor.capture_image().context("Failed to capture screen for time-lapse")?;
                        let final_image = if settings_for_capture.screenshot_region_mode == ScreenshotRegionMode::Custom {
                            crop_image(&image, &settings_for_capture.custom_region)
                        } else {
                            image
                        };
                        write_png_atomic(&output_for_closure, &final_image)?;
                        Ok(())
                    });
                    if result.is_ok() && output.exists() {
                        let s = settings_snapshot.clone();
                        let _ = post_process_capture_inner(&s, &output);
                        emit_file_created(&app_handle, &output);
                        *last_capture_ref.lock() = Some(output.to_string_lossy().to_string());
                        outputs.push(output);
                    } else if let Err(e) = result {
                        eprintln!("[defEYE] Time-lapse screen capture error: {e}");
                    }
                }
                _ => {}
            }

            match target {
                TimeLapseTarget::Webcam | TimeLapseTarget::Both => {
                    let output = session_folder.join(format!("defEYE_timelapse_webcam_{}.png", frame_ts));
                    let device = effective_camera_device(&settings_snapshot);
                    if device.trim().is_empty() {
                        eprintln!("[defEYE] Time-lapse: no camera device selected");
                    } else {
                        let input = normalize_dshow_input(&device);
                        let out_arg = output.to_string_lossy().to_string();
                        let status = ffmpeg_command()
                            .args(["-y", "-f", "dshow", "-rtbufsize", "50M", "-i", &input, "-frames:v", "1", "-vf", "scale=1280:-1", "-f", "image2", &out_arg])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                        let ok = matches!(status, Ok(s) if s.success());
                        if ok && output.exists() {
                            let s = settings_snapshot.clone();
                            let _ = post_process_capture_inner(&s, &output);
                            emit_file_created(&app_handle, &output);
                            *last_capture_ref.lock() = Some(output.to_string_lossy().to_string());
                            outputs.push(output);
                        } else if !ok {
                            eprintln!("[defEYE] Time-lapse webcam capture failed");
                        }
                    }
                }
                _ => {}
            }

            if !outputs.is_empty() {
                let last = outputs.last().unwrap();
                let _ = app_handle.emit("timelapse-status", timelapse_status_payload_from(
                    active.load(Ordering::SeqCst),
                    interval,
                    &target,
                    last,
                ));
            }

            for _ in 0..interval {
                if !active.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
        eprintln!("[defEYE] Time-lapse stopped");
    });

    let _ = app.emit("timelapse-status", timelapse_status_payload(state));
    Ok(())
}

#[tauri::command]
fn stop_timelapse(app: AppHandle, state: State<'_, AppState>) -> std::result::Result<(), String> {
    stop_timelapse_inner(&app, state.inner());
    Ok(())
}

fn stop_timelapse_inner(app: &AppHandle, state: &AppState) {
    state.timelapse_active.store(false, Ordering::SeqCst);
    let _ = app.emit("timelapse-stopped", timelapse_status_payload(state));
    let _ = app.emit("timelapse-status", timelapse_status_payload(state));
}

#[tauri::command]
fn get_timelapse_status(state: State<'_, AppState>) -> std::result::Result<TimelapseStatusPayload, String> {
    Ok(timelapse_status_payload(state.inner()))
}

fn timelapse_target_str(target: TimeLapseTarget) -> &'static str {
    match target {
        TimeLapseTarget::Screen => "screen",
        TimeLapseTarget::Webcam => "webcam",
        TimeLapseTarget::Both => "both",
    }
}

fn timelapse_status_payload(state: &AppState) -> TimelapseStatusPayload {
    let settings = state.settings.lock();
    TimelapseStatusPayload {
        active: state.timelapse_active.load(Ordering::SeqCst),
        interval_seconds: settings.timelapse_interval_seconds,
        target: timelapse_target_str(settings.timelapse_target).to_string(),
        last_capture: state.timelapse_last_capture.lock().clone(),
    }
}

fn timelapse_status_payload_from(
    active: bool,
    interval: u32,
    target: &TimeLapseTarget,
    last_capture: &Path,
) -> TimelapseStatusPayload {
    TimelapseStatusPayload {
        active,
        interval_seconds: interval,
        target: timelapse_target_str(*target).to_string(),
        last_capture: Some(last_capture.to_string_lossy().to_string()),
    }
}

// ---------------------------------------------------------------------------
// Recording Duration
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_recording_duration(state: State<'_, AppState>) -> std::result::Result<Option<u64>, String> {
    let webcam_start = state.recording_start_time.lock().clone();
    let screen_start = state.screen_recording_start_time.lock().clone();
    let earliest = webcam_start.or(screen_start);
    Ok(earliest.map(|start| {
        SystemTime::now()
            .duration_since(start)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }))
}

// ---------------------------------------------------------------------------
// Sentinel Watchdog — ffmpeg crash detection and auto-recovery
// ---------------------------------------------------------------------------

fn spawn_watchdog(app: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(WATCHDOG_INTERVAL);

            let state = app.state::<AppState>();
            let settings = state.settings.lock().clone();
            if !settings.watchdog_enabled {
                continue;
            }
            drop(settings);

            // Check webcam recording
            let webcam_child_alive = state.recording_child.lock().is_some();
            let webcam_path = state.recording_output_path.lock().is_some();
            if state.is_recording.load(Ordering::SeqCst) && !webcam_child_alive && webcam_path {
                eprintln!("[defEYE] Watchdog: webcam recording process died unexpectedly, attempting recovery");
                let _ = app.emit("defeye-error", "Watchdog: webcam ffmpeg process crashed. Attempting recovery...");
                let settings = state.settings.lock().clone();
                if settings.multi_camera_mode == MultiCameraMode::Multi && !settings.multi_camera_devices.is_empty() {
                    let _ = start_multi_recording_inner(&app, state.inner(), &settings);
                } else {
                    let _ = start_recording_with_settings(&app, state.inner(), &settings);
                }
                refresh_recording_state_from_children(state.inner(), &app);
            }

            // Check screen recording
            let screen_child_alive = state.screen_recording_child.lock().is_some();
            let screen_path = state.screen_recording_output_path.lock().is_some();
            if !screen_child_alive && screen_path {
                eprintln!("[defEYE] Watchdog: screen recording process died unexpectedly, attempting recovery");
                let _ = app.emit("defeye-error", "Watchdog: screen ffmpeg process crashed. Attempting recovery...");
                let settings = state.settings.lock().clone();
                let _ = start_screen_recording_with_settings(&app, state.inner(), &settings);
                refresh_recording_state_from_children(state.inner(), &app);
            }
        }
    });
}

fn exit_app_inner(app: &AppHandle, state: &AppState) -> Result<()> {
    let settings = state.settings.lock().clone();
    stop_motion_detection_blocking(state);

    let webcam_path = state.recording_output_path.lock().take();
    let screen_path = state.screen_recording_output_path.lock().take();
    let multi_paths: Vec<_> = state.multi_recording_output_paths.lock().drain(..).collect();
    let webcam_child = state.recording_child.lock().take();
    let screen_child = state.screen_recording_child.lock().take();
    let multi_children: Vec<_> = state.multi_recording_children.lock().drain(..).collect();

    if let Some(mut c) = webcam_child { graceful_stop_child(&mut c); }
    if let Some(mut c) = screen_child { graceful_stop_child(&mut c); }
    for mut c in multi_children { graceful_stop_child(&mut c); }

    if let Some(path) = webcam_path { post_process_capture(app, &settings, &path); }
    if let Some(path) = screen_path { post_process_capture(app, &settings, &path); }
    for path in multi_paths { post_process_capture(app, &settings, &path); }

    app.exit(0);
    Ok(())
}

#[tauri::command]
fn exit_app(app: AppHandle, state: State<'_, AppState>) -> std::result::Result<(), String> {
    exit_app_inner(&app, state.inner()).map_err(to_user_error)
}

#[tauri::command]
fn set_system_tray_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> std::result::Result<(), String> {
    {
        let mut settings = state.settings.lock();
        settings.system_tray_enabled = enabled;
    }
    let settings = state.settings.lock().clone();
    save_settings(&app, &settings).map_err(to_user_error)?;

    if enabled {
        if app.tray_by_id("defeye-tray").is_none() {
            build_system_tray(&app).map_err(to_user_error)?;
        } else if let Some(tray) = app.tray_by_id("defeye-tray") {
            let _ = tray.set_visible(true);
        }
    } else if let Some(tray) = app.tray_by_id("defeye-tray") {
        let _ = tray.set_visible(false);
    }
    Ok(())
}

#[tauri::command]
fn update_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    action_id: String,
    new_shortcut: String,
) -> std::result::Result<(), String> {
    let action = action_id_to_enum(&action_id)
        .ok_or_else(|| format!("Unknown hotkey action: {action_id}"))?;

    let settings = state.settings.lock().clone();
    let old_shortcut = get_hotkey_by_action_id(&settings, &action_id)
        .ok_or_else(|| format!("Unknown hotkey action: {action_id}"))?;

    // Check for conflicts with other actions
    for s in all_hotkey_shortcuts(&settings) {
        if s.eq_ignore_ascii_case(&new_shortcut) && !s.eq_ignore_ascii_case(old_shortcut) {
            return Err(format!("Shortcut '{new_shortcut}' is already bound to another action"));
        }
    }

    // Unregister old shortcut
    let _ = app.global_shortcut().unregister(old_shortcut);

    // Register new shortcut
    app.global_shortcut()
        .on_shortcut(new_shortcut.as_str(), move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                thread::spawn(move || {
                    handle_hotkey(app, action);
                });
            }
        })
        .map_err(|e| format!("Failed to register shortcut '{new_shortcut}': {e}"))?;

    // Save to settings
    let mut new_settings = settings;
    set_hotkey_by_action_id(&mut new_settings, &action_id, &new_shortcut);
    save_settings(&app, &new_settings).map_err(to_user_error)?;
    *state.settings.lock() = new_settings;

    let _ = app.emit("settings-updated", state.settings.lock().clone());

    Ok(())
}

#[tauri::command]
fn reset_hotkeys(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<(), String> {
    let settings = state.settings.lock().clone();

    // Unregister all existing shortcuts
    for s in all_hotkey_shortcuts(&settings) {
        let _ = app.global_shortcut().unregister(s.as_str());
    }

    // Reset to defaults
    let mut new_settings = settings;
    new_settings.hotkeys = HotkeySettings::default();

    // Re-register with defaults
    register_global_shortcuts(&app, &new_settings).map_err(to_user_error)?;

    // Save
    save_settings(&app, &new_settings).map_err(to_user_error)?;
    *state.settings.lock() = new_settings;

    let _ = app.emit("settings-updated", state.settings.lock().clone());

    Ok(())
}

// ---------------------------------------------------------------------------
// Voice Control — cpal audio capture + vosk offline speech recognition
// ---------------------------------------------------------------------------

/// Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();
    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

/// Similarity ratio between two strings (0.0 = completely different, 1.0 = identical).
/// Based on Levenshtein distance normalized by the longer string's length.
fn similarity_ratio(a: &str, b: &str) -> f32 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 { return 1.0; }
    let dist = levenshtein(a, b);
    1.0 - (dist as f32 / max_len as f32)
}

fn vosk_dll_available() -> bool {
    vosk_dynamic::is_available()
}

fn start_voice_monitoring(app: AppHandle, state: &AppState) -> Result<()> {
    if state.voice_active.load(Ordering::SeqCst) {
        return Ok(());
    }

    if !vosk_dll_available() {
        bail!("Voice control is unavailable — libvosk.dll was not found next to the executable. Voice features are disabled.");
    }

    let settings = state.settings.lock().clone();
    let model_path = settings.voice_model_path.trim().to_string();
    if model_path.is_empty() {
        bail!("Vosk model path is not set. Download a model (e.g. vosk-model-small-en-us-0.15) and set the path in Voice settings.");
    }
    if !Path::new(&model_path).exists() {
        bail!("Vosk model path does not exist: {}", model_path);
    }

    let audio_device_name = settings.voice_audio_device.clone();
    // Build grammar list for the grammar-based recognizer (used for command matching).
    // Includes command phrases, wake word, and wake_word+command combos.
    let wake_word_for_grammar = settings.voice_wake_word.trim().to_lowercase();
    let commands_for_grammar = settings.voice_commands.clone();
    let mut grammar_list: Vec<String> = Vec::new();
    for cmd in &commands_for_grammar {
        let phrase = cmd.phrase.to_lowercase();
        grammar_list.push(phrase.clone());
        if !wake_word_for_grammar.is_empty() {
            grammar_list.push(format!("{} {}", wake_word_for_grammar, phrase));
        }
    }
    if !wake_word_for_grammar.is_empty() {
        grammar_list.push(wake_word_for_grammar.clone());
    }

    let voice_active = state.voice_active.clone();
    let voice_last_command = state.voice_last_command.clone();
    let voice_last_command_time = state.voice_last_command_time.clone();
    let app_handle = app.clone();

    let handle = thread::spawn(move || {
        // Load vosk model inside the thread (Model may not be Send)
        let model = match Model::new(&model_path) {
            Some(m) => m,
            None => {
                eprintln!("[defEYE] Voice: failed to load vosk model");
                let _ = app_handle.emit("defeye-error", "Failed to load Vosk model".to_string());
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };

        // Set up cpal audio input
        let host = cpal::default_host();
        let device = if audio_device_name.is_empty() {
            host.default_input_device()
                .ok_or_else(|| anyhow!("No default input device available"))
        } else {
            host.input_devices()
                .ok()
                .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(&audio_device_name)))
                .ok_or_else(|| anyhow!("Audio device '{}' not found", audio_device_name))
        };

        let device = match device {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[defEYE] Voice: audio device error: {e}");
                let _ = app_handle.emit("defeye-error", e.to_string());
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };

        let supported_config = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[defEYE] Voice: failed to get default input config: {e}");
                let _ = app_handle.emit("defeye-error", format!("Failed to get audio config: {e}"));
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };

        // Use the device's native config — vosk needs 16kHz mono i16, we convert in the callback
        let input_sample_rate = supported_config.sample_rate().0;
        let input_channels = supported_config.channels() as usize;
        let sample_format = supported_config.sample_format();
        eprintln!("[defEYE] Voice: device native config: {}Hz, {}ch, {:?}", input_sample_rate, input_channels, sample_format);

        // Build the stream using the device's supported config
        let stream_config = supported_config.clone().into();

        // Free-form recognizer for full transcription (all words)
        let free_recognizer = Recognizer::new(&model, 16000.0);
        // Grammar-based recognizer for high-accuracy command matching
        let grammar_recognizer = if grammar_list.is_empty() {
            Recognizer::new(&model, 16000.0)
        } else {
            let grammar_refs: Vec<&str> = grammar_list.iter().map(|s| s.as_str()).collect();
            Recognizer::new_with_grammar(&model, 16000.0, &grammar_refs)
        };

        let mut free_recognizer = match free_recognizer {
            Some(r) => r,
            None => {
                eprintln!("[defEYE] Voice: failed to create free-form recognizer");
                let _ = app_handle.emit("defeye-error", "Failed to create Vosk recognizer".to_string());
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };
        let mut grammar_recognizer = match grammar_recognizer {
            Some(r) => r,
            None => {
                eprintln!("[defEYE] Voice: failed to create grammar recognizer");
                let _ = app_handle.emit("defeye-error", "Failed to create Vosk recognizer".to_string());
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };

        free_recognizer.set_max_alternatives(0);
        free_recognizer.set_words(true);
        free_recognizer.set_partial_words(false);
        grammar_recognizer.set_max_alternatives(0);
        grammar_recognizer.set_words(true);
        grammar_recognizer.set_partial_words(false);

        let free_recognizer = Arc::new(StdMutex::new(free_recognizer));
        let grammar_recognizer = Arc::new(StdMutex::new(grammar_recognizer));
        let app_for_callback = app_handle.clone();
        let voice_active_cb = voice_active.clone();
        let voice_last_cmd_cb = voice_last_command.clone();
        let voice_last_cmd_time_cb = voice_last_command_time.clone();

        let stream = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !voice_active_cb.load(Ordering::SeqCst) {
                    return;
                }

                // Downmix to mono (average all channels)
                let mono: Vec<f32> = if input_channels > 1 {
                    data.chunks(input_channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / input_channels as f32)
                        .collect()
                } else {
                    data.to_vec()
                };

                // Resample to 16kHz if needed (linear interpolation)
                let samples_i16: Vec<i16> = if input_sample_rate != 16000 {
                    let ratio = 16000.0 / input_sample_rate as f32;
                    let out_len = (mono.len() as f32 * ratio) as usize;
                    let mut out = Vec::with_capacity(out_len);
                    for i in 0..out_len {
                        let src_idx = i as f32 / ratio;
                        let idx0 = src_idx as usize;
                        let idx1 = (idx0 + 1).min(mono.len().saturating_sub(1));
                        let frac = src_idx - idx0 as f32;
                        let sample = mono[idx0] * (1.0 - frac) + mono[idx1] * frac;
                        out.push((sample * 32767.0).clamp(-32768.0, 32767.0) as i16);
                    }
                    out
                } else {
                    mono.iter().map(|s| (*s * 32767.0).clamp(-32768.0, 32767.0) as i16).collect()
                };

                // Feed audio to both recognizers
                let mut free_rec = free_recognizer.lock().unwrap();
                let free_state = free_rec.accept_waveform(&samples_i16);

                let mut gram_rec = grammar_recognizer.lock().unwrap();
                let gram_state = gram_rec.accept_waveform(&samples_i16);

                // Use free-form partial for live transcript display
                if free_state != vosk_dynamic::DecodingState::Finalized {
                    let partial = free_rec.partial_result();
                    let partial_text = partial.partial.trim().to_string();
                    drop(free_rec);
                    drop(gram_rec);
                    if !partial_text.is_empty() {
                        let _ = app_for_callback.emit("voice-transcript", &partial_text);
                    }
                    return;
                }

                // Free-form finalized — extract full transcript text
                let free_result = free_rec.result();
                let free_text: String = if let vosk_dynamic::CompleteResult::Single(single) = free_result {
                    single.text.to_string()
                } else {
                    String::new()
                };
                drop(free_rec);

                // Grammar finalized — use for command matching (high accuracy)
                let gram_text: String = if gram_state == vosk_dynamic::DecodingState::Finalized {
                    let gram_result = gram_rec.result();
                    if let vosk_dynamic::CompleteResult::Single(single) = gram_result {
                        single.text.to_string()
                    } else {
                        String::new()
                    }
                } else {
                    // Grammar didn't finalize — use partial
                    let partial = gram_rec.partial_result();
                    partial.partial.trim().to_string()
                };
                drop(gram_rec);

                let transcript_text = free_text.trim();
                if transcript_text.is_empty() { return; }
                let transcript_lower = transcript_text.to_lowercase();
                eprintln!("[defEYE] Voice transcript: {transcript_lower}");

                // For command matching, try grammar first, then fall back to free-form.
                // The grammar recognizer is built at startup with the initial command set,
                // so if the user changed themes/commands, the grammar may be stale.
                // We try both and pick whichever produces a command match.
                let grammar_lower = gram_text.trim().to_lowercase();
                if !grammar_lower.is_empty() {
                    eprintln!("[defEYE] Voice grammar match: {grammar_lower}");
                }

                // Read live settings (wake word, commands, confidence) so changes take effect immediately
                let live_settings = app_for_callback
                    .state::<AppState>()
                    .settings
                    .lock()
                    .clone();
                let confidence_threshold = live_settings.voice_confidence_threshold;
                let live_wake_word = live_settings.voice_wake_word.clone();
                let live_commands = live_settings.voice_commands.clone();

                // Try matching with grammar result first, then free-form transcript
                let candidates: Vec<&str> = if !grammar_lower.is_empty() && grammar_lower != transcript_lower {
                    vec![grammar_lower.as_str(), transcript_lower.as_str()]
                } else if !grammar_lower.is_empty() {
                    vec![grammar_lower.as_str()]
                } else {
                    vec![transcript_lower.as_str()]
                };

                // Check wake word with confidence-based fuzzy matching on each candidate
                // Returns (cmd_text, matched_text) for the first candidate that passes wake word check
                let mut cmd_text: Option<String> = None;
                let mut matched_source: &str = "";

                for candidate in &candidates {
                    if live_wake_word.is_empty() {
                        cmd_text = Some(candidate.to_string());
                        matched_source = candidate;
                        break;
                    } else {
                        let ww = live_wake_word.to_lowercase();
                        let ww_words: Vec<&str> = ww.split_whitespace().collect();
                        let text_words: Vec<&str> = candidate.split_whitespace().collect();

                        let mut best_pos: Option<(usize, f32)> = None;
                        if !ww_words.is_empty() && text_words.len() >= ww_words.len() {
                            for i in 0..=(text_words.len() - ww_words.len()) {
                                let segment = &text_words[i..i + ww_words.len()];
                                let segment_str = segment.join(" ");
                                let sim = similarity_ratio(&segment_str, &ww);
                                if sim >= confidence_threshold {
                                    if best_pos.map_or(true, |(_, s)| sim > s) {
                                        best_pos = Some((i, sim));
                                    }
                                }
                            }
                        }

                        if let Some((pos, sim)) = best_pos {
                            let after = &text_words[pos + ww_words.len()..];
                            let cmd = after.join(" ");
                            eprintln!("[defEYE] Voice: wake word matched (sim={:.2}) — command: '{}'", sim, cmd);
                            cmd_text = Some(cmd);
                            matched_source = candidate;
                            break;
                        }
                    }
                }

                let cmd_text = match cmd_text {
                    Some(c) if !c.is_empty() => c,
                    _ => {
                        if !live_wake_word.is_empty() {
                            eprintln!("[defEYE] Voice: wake word '{}' not found in any candidate (threshold={:.2})", live_wake_word, confidence_threshold);
                        }
                        let _ = app_for_callback.emit("voice-transcript", transcript_text);
                        emit_voice_status(&app_for_callback, "listening", None, None);
                        return;
                    }
                };
                let _ = matched_source; // used for logging above

                // Match against commands using similarity-based confidence.
                // Try all candidates (grammar result first, then free-form) so that
                // if the grammar is stale (e.g. user changed themes), the free-form
                // transcript can still match the live command list.
                let mut matched: Option<&VoiceCommand> = None;
                let mut best_sim: f32 = 0.0;
                let mut cmd_text_trimmed = cmd_text.trim();
                for cmd in live_commands.iter() {
                    let phrase = cmd.phrase.to_lowercase();
                    let phrase = phrase.trim();
                    let sim = similarity_ratio(cmd_text_trimmed, phrase);
                    if sim >= confidence_threshold && sim > best_sim {
                        best_sim = sim;
                        matched = Some(cmd);
                    }
                }
                // If grammar-derived cmd_text didn't match, try free-form transcript
                if matched.is_none() && candidates.len() > 1 {
                    cmd_text_trimmed = candidates[1].trim();
                    for cmd in live_commands.iter() {
                        let phrase = cmd.phrase.to_lowercase();
                        let phrase = phrase.trim();
                        let sim = similarity_ratio(cmd_text_trimmed, phrase);
                        if sim >= confidence_threshold && sim > best_sim {
                            best_sim = sim;
                            matched = Some(cmd);
                        }
                    }
                }
                if let Some(cmd) = matched {
                    eprintln!("[defEYE] Voice: command matched '{}' (sim={:.2})", cmd.phrase, best_sim);
                    let now = Local::now().to_rfc3339();
                    *voice_last_cmd_cb.lock() = Some(cmd.phrase.clone());
                    *voice_last_cmd_time_cb.lock() = Some(now.clone());
                    let _ = app_for_callback.emit("voice-command-executed", &cmd.phrase);
                    let _ = app_for_callback.emit("voice-transcript", transcript_text);
                    emit_voice_status(&app_for_callback, "command_detected", Some(cmd.phrase.clone()), Some(now));

                    execute_voice_action(&app_for_callback, &cmd.action);

                    let feedback_enabled = live_settings.voice_feedback;
                    if feedback_enabled {
                        let feedback_msg = action_feedback_message(&cmd.action);
                        speak_feedback(&feedback_msg);
                    }
                } else {
                    // No command matched — still show transcript
                    let _ = app_for_callback.emit("voice-transcript", transcript_text);
                    emit_voice_status(&app_for_callback, "listening", None, None);
                }
            },
            |err| {
                eprintln!("[defEYE] Voice stream error: {err}");
            },
            None,
        );

        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[defEYE] Voice: failed to build input stream: {e}");
                let _ = app_handle.emit("defeye-error", format!("Failed to open audio stream: {e}"));
                voice_active.store(false, Ordering::SeqCst);
                emit_voice_status(&app_handle, "error", None, None);
                return;
            }
        };

        if let Err(e) = stream.play() {
            eprintln!("[defEYE] Voice: failed to start stream: {e}");
            let _ = app_handle.emit("defeye-error", format!("Failed to start audio stream: {e}"));
            voice_active.store(false, Ordering::SeqCst);
            emit_voice_status(&app_handle, "error", None, None);
            return;
        }

        voice_active.store(true, Ordering::SeqCst);
        emit_voice_status(&app_handle, "listening", None, None);
        eprintln!("[defEYE] Voice monitoring started (device={}, model={})", audio_device_name, model_path);

        // Keep the stream alive while voice_active is true
        let _stream = stream; // keep alive
        while voice_active.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
        }

        // Explicitly stop the stream before dropping
        // On Windows, dropping a cpal stream without stopping can hang
        drop(_stream);

        // Stream is dropped here, stopping audio capture
        eprintln!("[defEYE] Voice monitoring stopped");
        emit_voice_status(&app_handle, "idle", None, None);
    });

    *state.voice_thread.lock() = Some(handle);
    Ok(())
}

fn stop_voice_monitoring(state: &AppState) {
    state.voice_active.store(false, Ordering::SeqCst);
    // The thread will see the flag and exit, dropping the stream
    let handle = state.voice_thread.lock().take();
    if let Some(h) = handle {
        // join() blocks until the thread exits; the thread checks voice_active
        // every 200ms so this should return quickly. If the stream drop hangs,
        // the Tauri command will appear slow but won't deadlock the app since
        // the audio callback checks voice_active and returns early.
        let _ = h.join();
    }
}

fn restart_voice_monitoring_async(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        stop_voice_monitoring(state.inner());
        thread::sleep(Duration::from_millis(300));
        if state.settings.lock().voice_control_enabled {
            if let Err(e) = start_voice_monitoring(app.clone(), state.inner()) {
                eprintln!("[defEYE] Failed to restart voice monitoring: {e}");
                let _ = app.emit("defeye-error", e.to_string());
            }
        }
    });
}

fn execute_voice_action(app: &AppHandle, action: &VoiceAction) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    match action {
        VoiceAction::StartWebcam => {
            eprintln!("[defEYE] Voice: starting webcam recording");
            let _ = start_recording_with_settings(app, state.inner(), &settings);
            refresh_recording_state_from_children(state.inner(), app);
        }
        VoiceAction::StopRecording => {
            eprintln!("[defEYE] Voice: stopping all recording");
            let _ = stop_all_recording_inner(app, state.inner());
            refresh_recording_state_from_children(state.inner(), app);
        }
        VoiceAction::CapturePrimary => {
            eprintln!("[defEYE] Voice: capturing primary screen");
            let _ = capture_current_inner(app, state.inner());
        }
        VoiceAction::CaptureAllMerged => {
            eprintln!("[defEYE] Voice: capturing all screens merged");
            let _ = capture_all_merged_inner(app, state.inner());
        }
        VoiceAction::ToggleMotion => {
            eprintln!("[defEYE] Voice: toggling motion mode");
            let _ = toggle_motion_mode_inner(app, state.inner());
        }
        VoiceAction::DisableMotion => {
            if state.motion_active.load(Ordering::SeqCst) || state.settings.lock().motion_mode_enabled {
                eprintln!("[defEYE] Voice: disabling motion mode");
                let _ = toggle_motion_mode_inner(app, state.inner());
            }
        }
        VoiceAction::ShowSettings => {
            eprintln!("[defEYE] Voice: toggling stealth");
            let _ = toggle_stealth_mode_inner(app).map(|_| ());
        }
        VoiceAction::StartScreenRecording => {
            eprintln!("[defEYE] Voice: starting screen recording");
            let _ = start_screen_recording_with_settings(app, state.inner(), &settings);
            refresh_recording_state_from_children(state.inner(), app);
        }
        VoiceAction::StopAllAndDisableMotion => {
            eprintln!("[defEYE] Voice: perimeter clear — stopping all and disabling motion");
            let _ = stop_all_recording_inner(app, state.inner());
            refresh_recording_state_from_children(state.inner(), app);
            if state.motion_active.load(Ordering::SeqCst) || state.settings.lock().motion_mode_enabled {
                let _ = toggle_motion_mode_inner(app, state.inner());
            }
        }
        VoiceAction::StopScreenRecording => {
            eprintln!("[defEYE] Voice: stopping screen recording");
            let _ = stop_screen_recording_inner(app, state.inner());
            refresh_recording_state_from_children(state.inner(), app);
        }
        VoiceAction::ToggleStealth => {
            eprintln!("[defEYE] Voice: toggling stealth mode");
            let _ = toggle_stealth_mode_inner(app).map(|_| ());
        }
        VoiceAction::StartTimelapse => {
            eprintln!("[defEYE] Voice: starting timelapse");
            let _ = start_timelapse_inner(app.clone(), state.inner());
        }
        VoiceAction::StopTimelapse => {
            eprintln!("[defEYE] Voice: stopping timelapse");
            stop_timelapse_inner(app, state.inner());
        }
        VoiceAction::CycleCameraLeft => {
            eprintln!("[defEYE] Voice: cycling camera left");
            let _ = cycle_camera_inner(app, state.inner(), -1);
        }
        VoiceAction::CycleCameraRight => {
            eprintln!("[defEYE] Voice: cycling camera right");
            let _ = cycle_camera_inner(app, state.inner(), 1);
        }
        VoiceAction::CaptureRegion => {
            eprintln!("[defEYE] Voice: starting region selector");
            if let Some(window) = app.get_webview_window(REGION_SELECTOR_LABEL) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        VoiceAction::OpenOutputFolder => {
            eprintln!("[defEYE] Voice: opening output folder");
            let output_dir = state.output_dir.lock().clone();
            let _ = open_path(app.clone(), output_dir.to_string_lossy().to_string());
        }
    }
}

fn action_feedback_message(action: &VoiceAction) -> String {
    match action {
        VoiceAction::StartWebcam => "Recording started".to_string(),
        VoiceAction::StopRecording => "Recording stopped".to_string(),
        VoiceAction::CapturePrimary => "Capture taken".to_string(),
        VoiceAction::CaptureAllMerged => "Merged capture taken".to_string(),
        VoiceAction::ToggleMotion => "Motion mode toggled".to_string(),
        VoiceAction::DisableMotion => "Motion mode disabled".to_string(),
        VoiceAction::ShowSettings => "Stealth toggled".to_string(),
        VoiceAction::StartScreenRecording => "Screen recording started".to_string(),
        VoiceAction::StopAllAndDisableMotion => "Perimeter clear".to_string(),
        VoiceAction::StopScreenRecording => "Screen recording stopped".to_string(),
        VoiceAction::ToggleStealth => "Stealth mode toggled".to_string(),
        VoiceAction::StartTimelapse => "Time-lapse started".to_string(),
        VoiceAction::StopTimelapse => "Time-lapse stopped".to_string(),
        VoiceAction::CycleCameraLeft => "Camera cycled left".to_string(),
        VoiceAction::CycleCameraRight => "Camera cycled right".to_string(),
        VoiceAction::CaptureRegion => "Region selector opened".to_string(),
        VoiceAction::OpenOutputFolder => "Output folder opened".to_string(),
    }
}

fn speak_feedback(text: &str) {
    let escaped = text.replace('\'', "\\'");
    let script = format!(
        r#"
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.Rate = 2
$s.Speak('{}')
$s.Dispose()
"#,
        escaped
    );

    let mut command = Command::new("powershell.exe");
    command.arg("-NoProfile");
    command.arg("-ExecutionPolicy");
    command.arg("Bypass");
    command.arg("-Command");
    command.arg(&script);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let _ = command.spawn();
}

fn emit_voice_status(app: &AppHandle, status: &str, last_command: Option<String>, last_command_time: Option<String>) {
    let state = app.state::<AppState>();
    let active = state.voice_active.load(Ordering::SeqCst);
    let cmd = last_command.or_else(|| state.voice_last_command.lock().clone());
    let cmd_time = last_command_time.or_else(|| state.voice_last_command_time.lock().clone());
    let _ = app.emit(
        "voice-status",
        VoiceStatusPayload {
            active,
            status: status.to_string(),
            last_command: cmd,
            last_command_time: cmd_time,
        },
    );
}

#[tauri::command]
fn toggle_voice_control(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<bool, String> {
    let mut settings = state.settings.lock().clone();
    settings.voice_control_enabled = !settings.voice_control_enabled;
    let now_enabled = settings.voice_control_enabled;

    if now_enabled {
        // Try to start voice monitoring first. If it fails, revert the toggle
        // so the user doesn't get stuck with voice_control_enabled=true but
        // no active monitoring (which would require two clicks to recover).
        if let Err(e) = start_voice_monitoring(app.clone(), state.inner()) {
            settings.voice_control_enabled = false;
            let _ = save_settings(&app, &settings);
            *state.settings.lock() = settings;
            return Err(format!("Failed to start voice monitoring: {}", e));
        }
        save_settings(&app, &settings).map_err(to_user_error)?;
        *state.settings.lock() = settings;
    } else {
        save_settings(&app, &settings).map_err(to_user_error)?;
        *state.settings.lock() = settings;
        // Set voice_active to false and emit idle status before the blocking join
        // so the frontend updates immediately
        state.voice_active.store(false, Ordering::SeqCst);
        emit_voice_status(&app, "idle", None, None);
        stop_voice_monitoring(state.inner());
    }
    Ok(now_enabled)
}

#[tauri::command]
fn get_voice_status(state: State<'_, AppState>) -> VoiceStatusPayload {
    VoiceStatusPayload {
        active: state.voice_active.load(Ordering::SeqCst),
        status: if state.voice_active.load(Ordering::SeqCst) { "listening".to_string() } else { "idle".to_string() },
        last_command: state.voice_last_command.lock().clone(),
        last_command_time: state.voice_last_command_time.lock().clone(),
    }
}

// ---------------------------------------------------------------------------
// Audio level monitoring — cpal RMS capture for real-time meter
// ---------------------------------------------------------------------------

fn find_cpal_input_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    if name.trim().is_empty() {
        return host.default_input_device();
    }
    let name_lower = name.to_lowercase();
    // Collect all input devices once
    let devices: Vec<cpal::Device> = host.input_devices().ok()?.collect();
    if devices.is_empty() {
        return host.default_input_device();
    }
    // Try exact match first
    if let Some(d) = devices.iter().find(|d| d.name().ok().as_deref() == Some(name)) {
        return Some(d.clone());
    }
    // Try case-insensitive exact match
    if let Some(d) = devices.iter().find(|d| {
        d.name()
            .ok()
            .map(|n| n.to_lowercase() == name_lower)
            .unwrap_or(false)
    }) {
        return Some(d.clone());
    }
    // Try substring match (either direction)
    if let Some(d) = devices.iter().find(|d| {
        d.name()
            .ok()
            .map(|n| {
                let n_lower = n.to_lowercase();
                n_lower.contains(&name_lower) || name_lower.contains(&n_lower)
            })
            .unwrap_or(false)
    }) {
        return Some(d.clone());
    }
    // Fallback: use default input device rather than failing entirely
    eprintln!("[defEYE] Audio level monitor: device '{}' not found in cpal, using default", name);
    host.default_input_device()
}

#[tauri::command]
fn start_audio_level_monitor(
    app: AppHandle,
    state: State<'_, AppState>,
    webcam_device: String,
    screen_device: String,
) -> std::result::Result<(), String> {
    stop_audio_level_monitor_inner(&state);

    state.audio_level_active.store(true, Ordering::SeqCst);
    let active_flag = state.audio_level_active.clone();

    let app_handle = app.clone();
    let webcam_level = Arc::new(AtomicU32::new(0));
    let screen_level = Arc::new(AtomicU32::new(0));

    let handle = thread::spawn(move || {
        let host = cpal::default_host();

        // Open webcam audio stream
        let webcam_stream = {
            let wl = webcam_level.clone();
            let af = active_flag.clone();
            let dev = find_cpal_input_device(&host, &webcam_device);
            match dev {
                Some(device) => {
                    match device.default_input_config() {
                        Ok(supported) => {
                            let config: cpal::StreamConfig = supported.clone().into();
                            match device.build_input_stream(
                                &config,
                                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                    if !af.load(Ordering::SeqCst) {
                                        return;
                                    }
                                    let rms = if data.is_empty() {
                                        0.0
                                    } else {
                                        let sum: f32 = data.iter().map(|s| s * s).sum();
                                        (sum / data.len() as f32).sqrt()
                                    };
                                    // Logarithmic scaling: maps quiet signals to visible levels
                                    // rms=0.001 -> ~0.33, rms=0.01 -> ~0.67, rms=0.05 -> ~0.87, rms=0.1+ -> ~1.0
                                    let level = if rms < 0.0001 {
                                        0.0
                                    } else {
                                        (rms.ln() / 5.0 + 1.0).clamp(0.0, 1.0)
                                    };
                                    wl.store((level * 10000.0) as u32, Ordering::Relaxed);
                                },
                                |err| {
                                    eprintln!("[defEYE] Audio level monitor (webcam) error: {err}");
                                },
                                None,
                            ) {
                                Ok(s) => {
                                    let _ = s.play();
                                    Some(s)
                                }
                                Err(e) => {
                                    eprintln!("[defEYE] Audio level monitor: failed to build webcam stream: {e}");
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[defEYE] Audio level monitor: no config for webcam device: {e}");
                            None
                        }
                    }
                }
                None => {
                    eprintln!("[defEYE] Audio level monitor: webcam device '{}' not found", webcam_device);
                    None
                }
            }
        };

        // Open screen audio stream
        let screen_stream = {
            let sl = screen_level.clone();
            let af = active_flag.clone();
            let dev = find_cpal_input_device(&host, &screen_device);
            match dev {
                Some(device) => {
                    match device.default_input_config() {
                        Ok(supported) => {
                            let config: cpal::StreamConfig = supported.clone().into();
                            match device.build_input_stream(
                                &config,
                                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                    if !af.load(Ordering::SeqCst) {
                                        return;
                                    }
                                    let rms = if data.is_empty() {
                                        0.0
                                    } else {
                                        let sum: f32 = data.iter().map(|s| s * s).sum();
                                        (sum / data.len() as f32).sqrt()
                                    };
                                    let level = if rms < 0.0001 {
                                        0.0
                                    } else {
                                        (rms.ln() / 5.0 + 1.0).clamp(0.0, 1.0)
                                    };
                                    sl.store((level * 10000.0) as u32, Ordering::Relaxed);
                                },
                                |err| {
                                    eprintln!("[defEYE] Audio level monitor (screen) error: {err}");
                                },
                                None,
                            ) {
                                Ok(s) => {
                                    let _ = s.play();
                                    Some(s)
                                }
                                Err(e) => {
                                    eprintln!("[defEYE] Audio level monitor: failed to build screen stream: {e}");
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[defEYE] Audio level monitor: no config for screen device: {e}");
                            None
                        }
                    }
                }
                None => {
                    eprintln!("[defEYE] Audio level monitor: screen device '{}' not found", screen_device);
                    None
                }
            }
        };

        // Emit levels periodically while active
        while active_flag.load(Ordering::SeqCst) {
            let wl = webcam_level.load(Ordering::Relaxed) as f32 / 10000.0;
            let sl = screen_level.load(Ordering::Relaxed) as f32 / 10000.0;
            let _ = app_handle.emit("audio-level", serde_json::json!({ "webcam": wl, "screen": sl }));
            thread::sleep(Duration::from_millis(33));
        }

        // Drop streams to stop capture
        drop(webcam_stream);
        drop(screen_stream);
        eprintln!("[defEYE] Audio level monitor stopped");
    });

    *state.audio_level_thread.lock() = Some(handle);
    Ok(())
}

fn stop_audio_level_monitor_inner(state: &AppState) {
    state.audio_level_active.store(false, Ordering::SeqCst);
    let handle = state.audio_level_thread.lock().take();
    if let Some(h) = handle {
        let _ = h.join();
    }
}

#[tauri::command]
fn stop_audio_level_monitor(state: State<'_, AppState>) {
    stop_audio_level_monitor_inner(&state);
}

#[tauri::command]
fn list_audio_input_devices() -> std::result::Result<Vec<AudioInputDevice>, String> {
    let host = cpal::default_host();
    let mut devices = Vec::new();
    if let Ok(input_devices) = host.input_devices() {
        for (i, device) in input_devices.enumerate() {
            let name = device.name().unwrap_or_else(|_| format!("Device {}", i));
            devices.push(AudioInputDevice {
                name,
                device_id: i.to_string(),
            });
        }
    }
    Ok(devices)
}

fn build_hidden_main_window(app: &AppHandle) -> Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App("index.html".into()))
        .title("defEYE - Sentinel")
        .decorations(false)
        .transparent(false)
        .skip_taskbar(true)
        .visible(false)
        .always_on_top(false)
        .resizable(true)
        .inner_size(1030.0, 975.0)
        .min_inner_size(875.0, 97.5)
        .max_inner_size(1375.0, 1200.0)
        .build()
        .context("Failed to create hidden settings window")?;

    let close_window = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = close_window.hide();
        }
    });

    Ok(window)
}

fn build_hud_window(app: &AppHandle, corner: HudCorner, minimal: bool) -> Result<WebviewWindow> {
    let initial_width = if minimal { 28.0 } else { 118.0 };
    let window = WebviewWindowBuilder::new(app, HUD_LABEL, WebviewUrl::App("index.html".into()))
        .title("defEYE HUD")
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .visible(true)
        .always_on_top(true)
        .resizable(false)
        .inner_size(initial_width, 28.0)
        .build()
        .context("Failed to create HUD window")?;

    let _ = window.set_ignore_cursor_events(true);
    position_hud_window(app, corner, minimal)?;
    if matches!(corner, HudCorner::Hidden) {
        let _ = window.hide();
    }
    Ok(window)
}

fn build_region_selector_window(app: &AppHandle) -> Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, REGION_SELECTOR_LABEL, WebviewUrl::App("index.html".into()))
        .title("Select Region")
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .visible(false)
        .always_on_top(true)
        .resizable(false)
        .inner_size(1920.0, 1080.0)
        .build()
        .context("Failed to create region selector window")?;

    let close_window = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = close_window.hide();
        }
    });

    Ok(window)
}

fn build_system_tray(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let webcam_active = state.recording_child.lock().is_some();
    let screen_active = state.screen_recording_child.lock().is_some();
    drop(state);

    let webcam_label = if webcam_active { "■  Stop Webcam Recording" } else { "▶  Start Webcam Recording" };
    let screen_label = if screen_active { "■  Stop Screen Recording" } else { "▶  Start Screen Recording" };

    let webcam_item = MenuItem::with_id(app, "toggle_webcam", webcam_label, true, None::<&str>)?;
    let screen_item = MenuItem::with_id(app, "toggle_screen", screen_label, true, None::<&str>)?;
    let stealth_item = MenuItem::with_id(app, "toggle_stealth", "Toggle Stealth", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide_tray", "Hide System Tray", true, None::<&str>)?;
    let kill_item = MenuItem::with_id(app, "kill", "Kill defEYE", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[
        &webcam_item,
        &screen_item,
        &stealth_item,
        &hide_item,
        &kill_item,
    ])?;

    let icon = app.default_window_icon()
        .context("No default window icon found")?
        .clone();

    let webcam_item_c = webcam_item.clone();
    let screen_item_c = screen_item.clone();

    TrayIconBuilder::with_id("defeye-tray")
        .icon(icon)
        .tooltip("defEYE Sentinel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "toggle_webcam" => {
                    let state = app.state::<AppState>();
                    let webcam_active = state.recording_child.lock().is_some();
                    if webcam_active {
                        let _ = stop_webcam_recording_inner(app, state.inner());
                        let _ = webcam_item_c.set_text("▶  Start Webcam Recording");
                    } else {
                        let _ = start_recording_inner(app, state.inner());
                        let _ = webcam_item_c.set_text("■  Stop Webcam Recording");
                    }
                }
                "toggle_screen" => {
                    let state = app.state::<AppState>();
                    let screen_active = state.screen_recording_child.lock().is_some();
                    if screen_active {
                        let _ = stop_screen_recording_inner(app, state.inner());
                        let _ = screen_item_c.set_text("▶  Start Screen Recording");
                    } else {
                        let _ = start_screen_recording_inner(app, state.inner());
                        let _ = screen_item_c.set_text("■  Stop Screen Recording");
                    }
                }
                "toggle_stealth" => {
                    let _ = toggle_stealth_mode_inner(app);
                }
                "hide_tray" => {
                    if let Some(tray) = app.tray_by_id("defeye-tray") {
                        let _ = tray.set_visible(false);
                    }
                    let mut settings = app.state::<AppState>().settings.lock().clone();
                    settings.system_tray_enabled = false;
                    let _ = save_settings(app, &settings);
                    *app.state::<AppState>().settings.lock() = settings.clone();
                    let _ = app.emit("settings-updated", settings);
                }
                "kill" => {
                    let state = app.state::<AppState>();
                    let _ = exit_app_inner(app, state.inner());
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window(MAIN_LABEL) {
                    let visible = w.is_visible().unwrap_or(false);
                    if visible {
                        let _ = w.hide();
                        if let Some(hud) = app.get_webview_window(HUD_LABEL) {
                            let _ = hud.hide();
                        }
                    } else {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_focus();
                        let state = app.state::<AppState>();
                        let settings = state.settings.lock();
                        if !matches!(settings.hud_corner, HudCorner::Hidden) {
                            if let Some(hud) = app.get_webview_window(HUD_LABEL) {
                                let _ = hud.unminimize();
                                let _ = hud.show();
                            }
                        }
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn position_hud_window(app: &AppHandle, corner: HudCorner, minimal: bool) -> Result<()> {
    let Some(window) = app.get_webview_window(HUD_LABEL) else {
        return Ok(());
    };

    if matches!(corner, HudCorner::Hidden) {
        let _ = window.hide();
        return Ok(());
    }
    let _ = window.show();

    let hud_width = if minimal { 28_i32 } else { 118_i32 };
    let hud_height = 28_i32;
    let _ = window.set_size(PhysicalSize {
        width: hud_width as u32,
        height: hud_height as u32,
    });

    let Some(monitor) = app
        .primary_monitor()
        .context("Failed to query primary monitor")?
    else {
        return Ok(());
    };

    let margin = 14_i32;
    let size = monitor.size();
    let origin = monitor.position();
    let monitor_width = i32::try_from(size.width).context("Primary monitor width is too large")?;
    let monitor_height =
        i32::try_from(size.height).context("Primary monitor height is too large")?;

    let x = match corner {
        HudCorner::TopLeft | HudCorner::BottomLeft | HudCorner::Hidden => origin.x + margin,
        HudCorner::TopRight | HudCorner::BottomRight => {
            origin.x + monitor_width - hud_width - margin
        }
    };
    let y = match corner {
        HudCorner::TopLeft | HudCorner::TopRight | HudCorner::Hidden => origin.y + margin,
        HudCorner::BottomLeft | HudCorner::BottomRight => {
            origin.y + monitor_height - hud_height - margin
        }
    };

    window
        .set_position(PhysicalPosition { x, y })
        .context("Failed to position HUD window")?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_ignore_cursor_events(true);
    Ok(())
}

fn register_global_shortcuts(app: &AppHandle, settings: &Settings) -> Result<()> {
    let h = &settings.hotkeys;
    register_one(app, &h.start_webcam, HotkeyAction::StartWebcam)?;
    register_one(app, &h.stop_webcam, HotkeyAction::StopWebcam)?;
    register_one(app, &h.start_screen, HotkeyAction::StartScreen)?;
    register_one(app, &h.stop_screen, HotkeyAction::StopScreen)?;
    register_one(app, &h.capture_current, HotkeyAction::CaptureCurrent)?;
    register_one(app, &h.capture_all_merged, HotkeyAction::CaptureAllMerged)?;
    register_one(app, &h.toggle_motion_mode, HotkeyAction::ToggleMotionMode)?;
    register_one(app, &h.cycle_camera_left, HotkeyAction::CycleCameraLeft)?;
    register_one(app, &h.cycle_camera_right, HotkeyAction::CycleCameraRight)?;
    register_one(app, &h.capture_region_selector, HotkeyAction::CaptureRegionSelector)?;
    register_one(app, &h.toggle_stealth, HotkeyAction::ToggleStealth)?;
    register_one(app, &h.toggle_timelapse, HotkeyAction::ToggleTimelapse)?;
    register_one(app, &h.kill_defeye, HotkeyAction::KillDefeye)?;
    Ok(())
}

fn register_one(app: &AppHandle, shortcut: &str, action: HotkeyAction) -> Result<()> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                thread::spawn(move || {
                    handle_hotkey(app, action);
                });
            }
        })
        .with_context(|| format!("Failed to register global shortcut {shortcut}"))
}

fn action_id_to_enum(id: &str) -> Option<HotkeyAction> {
    match id {
        "start_webcam" => Some(HotkeyAction::StartWebcam),
        "stop_webcam" => Some(HotkeyAction::StopWebcam),
        "start_screen" => Some(HotkeyAction::StartScreen),
        "stop_screen" => Some(HotkeyAction::StopScreen),
        "capture_current" => Some(HotkeyAction::CaptureCurrent),
        "capture_all_merged" => Some(HotkeyAction::CaptureAllMerged),
        "toggle_motion_mode" => Some(HotkeyAction::ToggleMotionMode),
        "cycle_camera_left" => Some(HotkeyAction::CycleCameraLeft),
        "cycle_camera_right" => Some(HotkeyAction::CycleCameraRight),
        "capture_region_selector" => Some(HotkeyAction::CaptureRegionSelector),
        "toggle_stealth" => Some(HotkeyAction::ToggleStealth),
        "toggle_timelapse" => Some(HotkeyAction::ToggleTimelapse),
        "kill_defeye" => Some(HotkeyAction::KillDefeye),
        _ => None,
    }
}

fn get_hotkey_by_action_id<'a>(settings: &'a Settings, id: &str) -> Option<&'a str> {
    let h = &settings.hotkeys;
    match id {
        "start_webcam" => Some(&h.start_webcam),
        "stop_webcam" => Some(&h.stop_webcam),
        "start_screen" => Some(&h.start_screen),
        "stop_screen" => Some(&h.stop_screen),
        "capture_current" => Some(&h.capture_current),
        "capture_all_merged" => Some(&h.capture_all_merged),
        "toggle_motion_mode" => Some(&h.toggle_motion_mode),
        "cycle_camera_left" => Some(&h.cycle_camera_left),
        "cycle_camera_right" => Some(&h.cycle_camera_right),
        "capture_region_selector" => Some(&h.capture_region_selector),
        "toggle_stealth" => Some(&h.toggle_stealth),
        "toggle_timelapse" => Some(&h.toggle_timelapse),
        "kill_defeye" => Some(&h.kill_defeye),
        _ => None,
    }
}

fn set_hotkey_by_action_id(settings: &mut Settings, id: &str, value: &str) {
    match id {
        "start_webcam" => settings.hotkeys.start_webcam = value.to_string(),
        "stop_webcam" => settings.hotkeys.stop_webcam = value.to_string(),
        "start_screen" => settings.hotkeys.start_screen = value.to_string(),
        "stop_screen" => settings.hotkeys.stop_screen = value.to_string(),
        "capture_current" => settings.hotkeys.capture_current = value.to_string(),
        "capture_all_merged" => settings.hotkeys.capture_all_merged = value.to_string(),
        "toggle_motion_mode" => settings.hotkeys.toggle_motion_mode = value.to_string(),
        "cycle_camera_left" => settings.hotkeys.cycle_camera_left = value.to_string(),
        "cycle_camera_right" => settings.hotkeys.cycle_camera_right = value.to_string(),
        "capture_region_selector" => settings.hotkeys.capture_region_selector = value.to_string(),
        "toggle_stealth" => settings.hotkeys.toggle_stealth = value.to_string(),
        "toggle_timelapse" => settings.hotkeys.toggle_timelapse = value.to_string(),
        "kill_defeye" => settings.hotkeys.kill_defeye = value.to_string(),
        _ => {}
    }
}

fn all_hotkey_shortcuts(settings: &Settings) -> Vec<String> {
    all_hotkey_shortcuts_from(&settings.hotkeys)
}

fn all_hotkey_shortcuts_from(h: &HotkeySettings) -> Vec<String> {
    vec![
        h.start_webcam.clone(),
        h.stop_webcam.clone(),
        h.start_screen.clone(),
        h.stop_screen.clone(),
        h.capture_current.clone(),
        h.capture_all_merged.clone(),
        h.toggle_motion_mode.clone(),
        h.cycle_camera_left.clone(),
        h.cycle_camera_right.clone(),
        h.capture_region_selector.clone(),
        h.toggle_stealth.clone(),
        h.toggle_timelapse.clone(),
        h.kill_defeye.clone(),
    ]
}

fn handle_hotkey(app: AppHandle, action: HotkeyAction) {
    let result = match action {
        HotkeyAction::StartWebcam => {
            let state = app.state::<AppState>();
            start_recording_inner(&app, state.inner()).map(|_| ())
        }
        HotkeyAction::StopWebcam => {
            let state = app.state::<AppState>();
            stop_webcam_recording_inner(&app, state.inner()).map(|_| ())
        }
        HotkeyAction::StopScreen => {
            let state = app.state::<AppState>();
            stop_screen_recording_inner(&app, state.inner()).map(|_| ())
        }
        HotkeyAction::StartScreen => {
            let state = app.state::<AppState>();
            start_screen_recording_inner(&app, state.inner()).map(|_| ())
        }
        HotkeyAction::CaptureCurrent => {
            let state = app.state::<AppState>();
            match capture_current_inner(&app, state.inner()) {
                Ok(path) => { let _ = app.emit("capture-toast", ("primary", path)); Ok(()) }
                Err(e) => Err(e),
            }
        }
        HotkeyAction::CaptureAllMerged => {
            let state = app.state::<AppState>();
            match capture_all_merged_inner(&app, state.inner()) {
                Ok(path) => { let _ = app.emit("capture-toast", ("merged", path)); Ok(()) }
                Err(e) => Err(e),
            }
        }
        HotkeyAction::ToggleMotionMode => {
            let state = app.state::<AppState>();
            match toggle_motion_mode_inner(&app, state.inner()) {
                Ok(enabled) => {
                    let msg = if enabled {
                        "Sentinel Motion Mode ENABLED"
                    } else {
                        "Sentinel Motion Mode DISABLED"
                    };
                    let _ = app.emit("motion-toggled", enabled);
                    eprintln!("{msg}");
                }
                Err(e) => {
                    let _ = app.emit("defeye-error", e.to_string());
                }
            }
            Ok(())
        }
        HotkeyAction::CycleCameraLeft => {
            let state = app.state::<AppState>();
            cycle_camera_inner(&app, state.inner(), -1).map(|_| ())
        }
        HotkeyAction::CycleCameraRight => {
            let state = app.state::<AppState>();
            cycle_camera_inner(&app, state.inner(), 1).map(|_| ())
        }
        HotkeyAction::CaptureRegionSelector => {
            start_region_selector(app.clone()).map_err(|e| anyhow!("{e}"))
        }
        HotkeyAction::ToggleStealth => {
            toggle_stealth_mode_inner(&app).map(|_| ())
        }
        HotkeyAction::ToggleTimelapse => {
            let state = app.state::<AppState>();
            let was_active = state.timelapse_active.load(Ordering::SeqCst);
            if was_active {
                stop_timelapse_inner(&app, state.inner());
                Ok(())
            } else {
                start_timelapse_inner(app.clone(), state.inner()).map_err(|e| anyhow!("{e}")).map(|_| {
                    let _ = app.emit("timelapse-status", timelapse_status_payload(state.inner()));
                })
            }
        }
        HotkeyAction::KillDefeye => {
            let state = app.state::<AppState>();
            exit_app(app.clone(), state).map_err(|e| anyhow!("{e}"))
        }
    };

    if let Err(error) = result {
        if let Some(state) = app.try_state::<AppState>() {
            emit_status(&app, state.inner());
        }
        let _ = app.emit("defeye-error", error.to_string());
    }
}

fn start_recording_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    ensure_not_finalizing(state)?;
    let settings = state.settings.lock().clone();

    // Disk Sentinel pre-check
    if !check_disk_space(&settings.output_dir, settings.disk_threshold_mb) {
        bail!("Disk space below {}MB threshold. Free up space or adjust the Disk Sentinel threshold.", settings.disk_threshold_mb);
    }

    // Multi-camera simultaneous recording mode
    if settings.multi_camera_mode == MultiCameraMode::Multi {
        if settings.multi_camera_devices.is_empty() {
            bail!("Multi-camera mode is selected but no cameras are checked. Select at least one camera in the Multi-Camera section.");
        }
        return start_multi_recording_inner(app, state, &settings);
    }

    if state.recording_child.lock().is_some() {
        return Ok("Webcam recording is already active".to_string());
    }

    let device = effective_camera_device(&settings);
    if device.trim().is_empty() {
        bail!("Failed to start recording: select a camera or enter a manual ffmpeg device string.");
    }
    ensure_dir(&settings.output_dir)?;

    let output = settings
        .output_dir
        .join(format!("defEYE_webcam_{}.mp4", timestamp()));
    let input = build_webcam_dshow_input(&settings, &device);
    eprintln!("[defEYE] dshow input string: {:?}", input);

    let has_audio = settings.webcam_audio_enabled && !settings.webcam_audio_device.trim().is_empty();
    let wm = build_watermark_filter(&settings, 1, if has_audio { Some(0) } else { None });
    let (eff_crf, _) = apply_recording_preset(settings.recording_preset, settings.crf, settings.screen_crf);

    let mut builder = FfmpegCommandBuilder::new()
        .dshow_input(&input, "100M")
        .watermark(wm, has_audio);

    if settings.embed_metadata {
        builder = builder.metadata("defEYE Capture", &format!("defEYE webcam capture {}", timestamp()));
    }

    let mut child = builder
        .x264_encoding(&eff_crf.to_string())
        .output(&output)
        .stdio_recording()
        .spawn_with_error("Failed to start recording")?;

    verify_ffmpeg_started(&mut child, "ffmpeg failed to start recording")?;

    *state.recording_child.lock() = Some(child);
    *state.recording_output_path.lock() = Some(output.clone());
    *state.recording_start_time.lock() = Some(SystemTime::now());
    refresh_recording_state(state);
    emit_status(app, state);
    spawn_auto_stop_thread(app, state, settings.max_recording_duration, false);
    eprintln!("[defEYE] Recording started, output path: {}", output.display());
    Ok(output.to_string_lossy().to_string())
}

fn stop_webcam_recording_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    // Take everything out of mutexes FIRST (releases locks immediately)
    let output_path = state.recording_output_path.lock().take();
    let child = state.recording_child.lock().take();
    let multi_children: Vec<_> = state.multi_recording_children.lock().drain(..).collect();
    let multi_paths: Vec<_> = state.multi_recording_output_paths.lock().drain(..).collect();
    *state.recording_start_time.lock() = None;

    let has_work = child.is_some() || !multi_children.is_empty() || output_path.is_some() || !multi_paths.is_empty();
    if has_work {
        begin_finalizing(state);
    }

    refresh_recording_state(state);
    emit_status(app, state);

    let settings = state.settings.lock().clone();
    let app_handle = app.clone();
    let output_path_for_log = output_path.clone();
    thread::spawn(move || {
        if let Some(mut c) = child {
            graceful_stop_child(&mut c);
        }
        for mut c in multi_children {
            graceful_stop_child(&mut c);
        }
        if let Some(ref path) = output_path_for_log {
            eprintln!("[defEYE] Recording stopped, checking file: {}", path.display());
            if path.exists() {
                eprintln!("[defEYE] File exists, size: {} bytes", fs::metadata(path).map(|m| m.len()).unwrap_or(0));
            } else {
                eprintln!("[defEYE] ERROR: File does not exist after stop!");
            }
            post_process_capture(&app_handle, &settings, path);
        }
        for path in &multi_paths {
            post_process_capture(&app_handle, &settings, path);
        }
        if has_work {
            finish_finalizing(&app_handle);
        }

        // Resume motion detection if it was paused for recording
        if settings.motion_mode_enabled {
            let state = app_handle.state::<AppState>();
            if !state.motion_active.load(Ordering::SeqCst) {
                eprintln!("[defEYE] Resuming motion detection after webcam recording stopped");
                let _ = start_motion_detection_inner(app_handle.clone(), state.inner());
            }
        }
    });

    Ok("Webcam recording stopped".to_string())
}

fn start_screen_recording_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    ensure_not_finalizing(state)?;
    if state.screen_recording_child.lock().is_some() {
        return Ok("Screen recording is already active".to_string());
    }

    let settings = state.settings.lock().clone();

    // Disk Sentinel pre-check
    if !check_disk_space(&settings.output_dir, settings.disk_threshold_mb) {
        bail!("Disk space below {}MB threshold. Free up space or adjust the Disk Sentinel threshold.", settings.disk_threshold_mb);
    }

    ensure_dir(&settings.output_dir)?;

    let output = settings
        .output_dir
        .join(format!("defEYE_screen_{}.mp4", timestamp()));
    let fps = settings.screen_fps.to_string();
    let (_, eff_screen_crf) = apply_recording_preset(settings.recording_preset, settings.crf, settings.screen_crf);
    let crf = eff_screen_crf.to_string();

    let gdigrab_args = build_gdigrab_input_args(&settings);

    let has_audio = settings.screen_audio_enabled && !settings.screen_audio_device.trim().is_empty();
    let wm_idx = if has_audio { 2 } else { 1 };
    let audio_idx = if has_audio { Some(1) } else { None };
    let wm = build_watermark_filter(&settings, wm_idx, audio_idx);

    let mut builder = FfmpegCommandBuilder::new()
        .gdigrab_input(&fps, "100M", &gdigrab_args);

    if has_audio {
        builder = builder.dshow_audio_input(settings.screen_audio_device.trim());
    }

    builder = builder.watermark(wm, has_audio);

    if settings.embed_metadata {
        builder = builder.metadata("defEYE Screen Capture", &format!("defEYE screen capture {}", timestamp()));
    }

    let mut child = builder
        .x264_encoding(&crf)
        .output(&output)
        .stdio_recording()
        .spawn_with_error("Failed to start screen recording")?;

    verify_ffmpeg_started(&mut child, "ffmpeg failed to start")?;

    *state.screen_recording_child.lock() = Some(child);
    *state.screen_recording_output_path.lock() = Some(output.clone());
    *state.screen_recording_start_time.lock() = Some(SystemTime::now());
    refresh_recording_state(state);
    emit_status(app, state);
    spawn_auto_stop_thread(app, state, settings.max_recording_duration, true);
    Ok(output.to_string_lossy().to_string())
}

fn stop_screen_recording_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    // Take everything out of mutexes FIRST
    let output_path = state.screen_recording_output_path.lock().take();
    let child = state.screen_recording_child.lock().take();
    *state.screen_recording_start_time.lock() = None;

    let has_work = child.is_some() || output_path.is_some();
    if has_work {
        begin_finalizing(state);
    }

    refresh_recording_state(state);
    emit_status(app, state);

    let settings = state.settings.lock().clone();
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Some(mut c) = child {
            graceful_stop_child(&mut c);
        }
        if let Some(ref path) = output_path {
            post_process_capture(&app_handle, &settings, path);
        }
        if has_work {
            finish_finalizing(&app_handle);
        }

        if settings.motion_mode_enabled {
            let state = app_handle.state::<AppState>();
            if !state.motion_active.load(Ordering::SeqCst) {
                eprintln!("[defEYE] Resuming motion detection after screen recording stopped");
                let _ = start_motion_detection_inner(app_handle.clone(), state.inner());
            }
        }
    });

    Ok("Screen recording stopped".to_string())
}

fn stop_all_recording_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    // Stop time-lapse if active
    if state.timelapse_active.load(Ordering::SeqCst) {
        stop_timelapse_inner(app, state);
    }
    // Take everything out of mutexes FIRST (releases locks immediately)
    let webcam_path = state.recording_output_path.lock().take();
    let screen_path = state.screen_recording_output_path.lock().take();
    let webcam_child = state.recording_child.lock().take();
    let screen_child = state.screen_recording_child.lock().take();
    let multi_children: Vec<_> = state.multi_recording_children.lock().drain(..).collect();
    let multi_paths: Vec<_> = state.multi_recording_output_paths.lock().drain(..).collect();
    *state.recording_start_time.lock() = None;
    *state.screen_recording_start_time.lock() = None;

    let has_work = webcam_child.is_some()
        || screen_child.is_some()
        || !multi_children.is_empty()
        || webcam_path.is_some()
        || screen_path.is_some()
        || !multi_paths.is_empty();
    if has_work {
        begin_finalizing(state);
    }

    refresh_recording_state(state);
    emit_status(app, state);

    let settings = state.settings.lock().clone();
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Some(mut c) = webcam_child {
            graceful_stop_child(&mut c);
        }
        if let Some(mut c) = screen_child {
            graceful_stop_child(&mut c);
        }
        for mut c in multi_children {
            graceful_stop_child(&mut c);
        }
        if let Some(ref path) = webcam_path {
            post_process_capture(&app_handle, &settings, path);
        }
        if let Some(ref path) = screen_path {
            post_process_capture(&app_handle, &settings, path);
        }
        for path in &multi_paths {
            post_process_capture(&app_handle, &settings, path);
        }
        if has_work {
            finish_finalizing(&app_handle);
        }

        // Resume motion detection if it was paused for recording
        if settings.motion_mode_enabled {
            let state = app_handle.state::<AppState>();
            if !state.motion_active.load(Ordering::SeqCst) {
                eprintln!("[defEYE] Resuming motion detection after recording stopped");
                let _ = start_motion_detection_inner(app_handle.clone(), state.inner());
            }
        }
    });

    Ok("Recording stopped".to_string())
}

fn begin_finalizing(state: &AppState) {
    state.finalizing_count.fetch_add(1, Ordering::SeqCst);
}

fn finish_finalizing(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.finalizing_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
        Some(count.saturating_sub(1))
    }).ok();
    emit_status(app, state.inner());
}

fn ensure_not_finalizing(state: &AppState) -> Result<()> {
    if state.finalizing_count.load(Ordering::SeqCst) > 0 {
        bail!("Previous recording is still finalizing. Try again once the HUD returns to idle.");
    }
    Ok(())
}

fn refresh_recording_state(state: &AppState) {
    let webcam_active = state.recording_child.lock().is_some();
    let screen_active = state.screen_recording_child.lock().is_some();
    let multi_active = !state.multi_recording_children.lock().is_empty();
    let is_recording = webcam_active || screen_active || multi_active;
    state.is_recording.store(is_recording, Ordering::SeqCst);
    *state.recording_kind.lock() = if !is_recording {
        "idle".to_string()
    } else if screen_active && (webcam_active || multi_active) {
        "mixed".to_string()
    } else if screen_active {
        "screen".to_string()
    } else {
        "webcam".to_string()
    };
}

fn capture_current_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    let settings = state.settings.lock().clone();
    ensure_dir(&settings.output_dir)?;

    let settings_for_thread = settings.clone();
    let image = run_with_com(move || {
        let monitor = selected_monitor(&settings_for_thread)?;
        let image = monitor
            .capture_image()
            .context("Failed to capture selected primary screen")?;
        Ok(image)
    })?;

    // Apply region cropping if in custom mode
    let final_image = if settings.screenshot_region_mode == ScreenshotRegionMode::Custom {
        crop_image(&image, &settings.custom_region)
    } else {
        image
    };

    let output = settings
        .output_dir
        .join(format!("defEYE_current_{}.png", timestamp()));
    write_png_atomic(&output, &final_image)?;

    if settings.watermark_enabled || settings.watermark_image_enabled {
        let mut image_to_process = final_image.clone();
        apply_png_watermark(&mut image_to_process, &settings);
        write_png_atomic(&output, &image_to_process)?;
    }

    post_process_capture_inner(&settings, &output)?;
    emit_file_created(app, &output);
    Ok(output.to_string_lossy().to_string())
}

fn capture_all_merged_inner(app: &AppHandle, state: &AppState) -> Result<String> {
    let settings = state.settings.lock().clone();
    let output_dir = settings.output_dir.clone();
    ensure_dir(&output_dir)?;

    let captured = run_with_com(|| {
        let monitors = Monitor::all().context("Failed to enumerate monitors")?;
        if monitors.is_empty() {
            bail!("Failed to capture screens: no monitors found.");
        }

        let mut captured = Vec::with_capacity(monitors.len());
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        for monitor in monitors {
            let x = monitor.x().context("Failed to read monitor x position")?;
            let y = monitor.y().context("Failed to read monitor y position")?;
            let image = monitor
                .capture_image()
                .context("Failed to capture monitor image")?;
            let width = i32::try_from(image.width()).context("Monitor image width is too large")?;
            let height = i32::try_from(image.height()).context("Monitor image height is too large")?;

            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + width);
            max_y = max_y.max(y + height);
            captured.push((image, x, y));
        }

        Ok((captured, min_x, min_y, max_x, max_y))
    })?;

    let (captured, min_x, min_y, max_x, max_y) = captured;

    let canvas_width = u32::try_from(max_x - min_x).context("Invalid merged canvas width")?;
    let canvas_height = u32::try_from(max_y - min_y).context("Invalid merged canvas height")?;
    let mut canvas = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(canvas_width, canvas_height);

    for (image, x, y) in captured {
        let target_x = u32::try_from(x - min_x).context("Invalid monitor x offset")?;
        let target_y = u32::try_from(y - min_y).context("Invalid monitor y offset")?;
        canvas
            .copy_from(&image, target_x, target_y)
            .context("Failed to merge monitor capture into canvas")?;
    }

    // Apply region cropping if in custom mode
    let final_canvas = if settings.screenshot_region_mode == ScreenshotRegionMode::Custom {
        crop_image(&canvas, &settings.custom_region)
    } else {
        canvas
    };

    let output = output_dir.join(format!("defEYE_allmerged_{}.png", timestamp()));
    write_png_atomic(&output, &final_canvas)?;

    if settings.watermark_enabled || settings.watermark_image_enabled {
        let mut image_to_process = final_canvas.clone();
        apply_png_watermark(&mut image_to_process, &settings);
        write_png_atomic(&output, &image_to_process)?;
    }

    post_process_capture_inner(&settings, &output)?;
    emit_file_created(app, &output);
    Ok(output.to_string_lossy().to_string())
}

fn delete_capture_files(target: &Path, output_dir: &Path) -> Result<()> {
    fs::remove_file(target).with_context(|| format!("Failed to delete {}", target.display()))?;

    for sidecar in [
        target.with_extension("sha256.json"),
        target.with_extension("meta.json"),
        target.with_extension("watermark.json"),
        target.with_extension("note.json"),
    ] {
        if sidecar.exists() {
            let _ = fs::remove_file(sidecar);
        }
    }

    if let Some(filename) = target.file_name().and_then(|name| name.to_str()) {
        let thumbnail = output_dir.join("thumbnails").join(format!("{filename}.png"));
        if thumbnail.exists() {
            let _ = fs::remove_file(thumbnail);
        }
    }

    // Clean up empty session folder if this was a timelapse capture
    if let Some(parent) = target.parent() {
        if parent.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("session_")).unwrap_or(false) {
            if let Ok(mut entries) = fs::read_dir(parent) {
                if entries.next().is_none() {
                    let _ = fs::remove_dir(parent);
                }
            }
        }
    }

    Ok(())
}

fn build_capture_info(path: &Path, metadata: &fs::Metadata, thumbs_dir: &Path, session: Option<&str>) -> Option<(SystemTime, CaptureInfo)> {
    let filename = path.file_name()?.to_str()?.to_string();
    let kind = capture_kind(&filename)?;
    let created_time = metadata
        .created()
        .or_else(|_| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let thumbnail_path = thumbs_dir.join(format!("{}.png", filename));
    let thumbnail = if thumbnail_path.exists() {
        Some(thumbnail_path.to_string_lossy().to_string())
    } else {
        None
    };

    let has_watermark = path.with_extension("watermark.json").exists();
    let has_integrity = path.with_extension("sha256.json").exists();
    let has_note = path.with_extension("note.json").exists();

    let is_video = kind == "webcam" || kind == "screen";
    let duration = if is_video { get_video_duration(path) } else { None };

    Some((
        created_time,
        CaptureInfo {
            path: path.to_string_lossy().to_string(),
            filename,
            kind: kind.to_string(),
            size: metadata.len(),
            created: format_system_time(created_time),
            thumbnail,
            has_watermark,
            has_integrity,
            has_note,
            session: session.map(|s| s.to_string()),
            duration,
        },
    ))
}

fn list_recent_captures_inner(state: &AppState) -> Result<Vec<CaptureInfo>> {
    let output_dir = state.output_dir.lock().clone();
    ensure_dir(&output_dir)?;
    let thumbs_dir = output_dir.join("thumbnails");
    let timelapse_dir = output_dir.join("timelapse");
    let mut captures: Vec<(SystemTime, CaptureInfo)> = Vec::new();

    // Scan main output directory (flat)
    for entry in fs::read_dir(&output_dir)
        .with_context(|| format!("Failed to read {}", output_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        if let Some(info) = build_capture_info(&path, &metadata, &thumbs_dir, None) {
            captures.push(info);
        }
    }

    // Scan timelapse session subfolders
    if timelapse_dir.exists() {
        for session_entry in fs::read_dir(&timelapse_dir)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();
            if !session_path.is_dir() {
                continue;
            }
            let session_name = session_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            for entry in fs::read_dir(&session_path)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let metadata = entry.metadata()?;
                if let Some(info) = build_capture_info(&path, &metadata, &thumbs_dir, Some(&session_name)) {
                    captures.push(info);
                }
            }
        }
    }

    captures.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(captures
        .into_iter()
        .take(100)
        .map(|(_, info)| info)
        .collect())
}

fn get_capture_stats_inner(output_dir: &Path) -> Result<CaptureStats> {
    ensure_dir(output_dir)?;
    let mut total_count = 0usize;
    let mut total_size = 0u64;
    let mut webcam_count = 0usize;
    let mut screen_count = 0usize;
    let mut multi_count = 0usize;
    let mut image_count = 0usize;
    let mut timelapse_count = 0usize;
    let mut oldest: Option<SystemTime> = None;
    let mut newest: Option<SystemTime> = None;
    let mut largest_capture: u64 = 0u64;
    let mut video_files: Vec<PathBuf> = Vec::new();

    let timelapse_dir = output_dir.join("timelapse");

    // Scan main output dir
    let mut scan_path = |dir: &Path| -> Result<()> {
        for entry in fs::read_dir(dir)
            .with_context(|| format!("Failed to read {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(kind) = capture_kind(filename) else {
                continue;
            };
            let metadata = entry.metadata()?;
            let created = metadata
                .created()
                .or_else(|_| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            total_count += 1;
            total_size += metadata.len();
            if metadata.len() > largest_capture {
                largest_capture = metadata.len();
            }

            match kind {
                "webcam" => { webcam_count += 1; video_files.push(path.clone()); },
                "screen" => { screen_count += 1; video_files.push(path.clone()); },
                "multi" => { multi_count += 1; video_files.push(path.clone()); },
                "timelapse" => timelapse_count += 1,
                "current" | "merged" => image_count += 1,
                _ => {}
            }

            if oldest.map_or(true, |o| created < o) {
                oldest = Some(created);
            }
            if newest.map_or(true, |n| created > n) {
                newest = Some(created);
            }
        }
        Ok(())
    };

    scan_path(output_dir)?;
    // Scan timelapse session subfolders
    if timelapse_dir.exists() {
        for session_entry in fs::read_dir(&timelapse_dir)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();
            if session_path.is_dir() {
                let _ = scan_path(&session_path);
            }
        }
    }

    // Compute total video duration via ffprobe
    let mut total_video_duration_secs = 0.0f64;
    for video_path in &video_files {
        if let Ok(output) = ffprobe_command()
            .args([
                "-v", "error",
                "-show_entries", "format=duration",
                "-of", "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(video_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(d) = stdout.trim().parse::<f64>() {
                total_video_duration_secs += d;
            }
        }
    }

    let video_count = webcam_count + screen_count + multi_count;
    let video_percentage = if total_count > 0 {
        (video_count as f64 / total_count as f64) * 100.0
    } else {
        0.0
    };

    Ok(CaptureStats {
        total_count,
        total_size_bytes: total_size,
        webcam_count,
        screen_count,
        multi_count,
        image_count,
        timelapse_count,
        oldest: oldest.map(format_system_time),
        newest: newest.map(format_system_time),
        total_video_duration_secs,
        largest_capture_bytes: largest_capture,
        video_percentage,
    })
}

/// Apply a recording preset to override CRF and screen CRF values.
/// Returns (crf, screen_crf) adjusted by the preset.
fn apply_recording_preset(preset: RecordingPreset, crf: u8, screen_crf: u8) -> (u8, u8) {
    match preset {
        RecordingPreset::Ultra => (18, 18),
        RecordingPreset::High => (20, 20),
        RecordingPreset::Medium => (crf, screen_crf),
        RecordingPreset::Low => (28, 28),
        RecordingPreset::Custom => (crf, screen_crf),
    }
}

/// Spawn a background thread that auto-stops a recording after max_duration seconds.
/// `is_screen` determines whether to stop the screen or webcam recording.
fn spawn_auto_stop_thread(app: &AppHandle, state: &AppState, max_duration: u32, is_screen: bool) {
    if max_duration == 0 {
        return;
    }
    let app_handle = app.clone();
    let screen_child = state.screen_recording_child.clone();
    let webcam_child = state.recording_child.clone();
    let screen_path = state.screen_recording_output_path.clone();
    let webcam_path = state.recording_output_path.clone();
    let settings = state.settings.lock().clone();
    let auto_restart = settings.auto_restart_recording;

    thread::spawn(move || {
        thread::sleep(Duration::from_secs(max_duration as u64));

        if is_screen {
            let child = screen_child.lock().take();
            let path = screen_path.lock().take();
            if let Some(mut c) = child {
                eprintln!("[defEYE] Auto-stop: stopping screen recording after {max_duration}s");
                let st = app_handle.state::<AppState>();
                begin_finalizing(st.inner());
                graceful_stop_child(&mut c);
                let st = app_handle.state::<AppState>();
                refresh_recording_state(st.inner());
                emit_status(&app_handle, st.inner());
                if let Some(p) = path {
                    post_process_capture(&app_handle, &settings, &p);
                }
                finish_finalizing(&app_handle);

                if auto_restart {
                    eprintln!("[defEYE] Auto-restart: restarting screen recording");
                    thread::sleep(Duration::from_secs(1));
                    let st = app_handle.state::<AppState>();
                    let _ = start_screen_recording_with_settings(&app_handle, st.inner(), &settings);
                    refresh_recording_state_from_children(st.inner(), &app_handle);
                }
            }
        } else {
            let child = webcam_child.lock().take();
            let path = webcam_path.lock().take();
            if let Some(mut c) = child {
                eprintln!("[defEYE] Auto-stop: stopping webcam recording after {max_duration}s");
                let st = app_handle.state::<AppState>();
                begin_finalizing(st.inner());
                graceful_stop_child(&mut c);
                let st = app_handle.state::<AppState>();
                refresh_recording_state(st.inner());
                emit_status(&app_handle, st.inner());
                if let Some(p) = path {
                    post_process_capture(&app_handle, &settings, &p);
                }
                finish_finalizing(&app_handle);

                if auto_restart {
                    eprintln!("[defEYE] Auto-restart: restarting webcam recording");
                    thread::sleep(Duration::from_secs(1));
                    let st = app_handle.state::<AppState>();
                    let _ = start_recording_inner(&app_handle, st.inner());
                    refresh_recording_state_from_children(st.inner(), &app_handle);
                }
            }
        }
    });
}

static FFMPEG_DSHOW_LOCK: StdMutex<()> = StdMutex::new(());

fn run_dshow_list_devices() -> Result<String> {
    let _guard = FFMPEG_DSHOW_LOCK.lock().ok();
    let output = ffmpeg_command()
        .args(["-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!("Failed to list devices: ffmpeg not found in PATH. Please install ffmpeg.")
            } else {
                anyhow!("Failed to list devices: {error}")
            }
        })?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(text)
}

fn list_cameras_inner() -> Result<Vec<String>> {
    let text = run_dshow_list_devices()?;
    Ok(parse_dshow_video_devices(&text))
}

fn parse_dshow_video_devices(text: &str) -> Vec<String> {
    let mut devices = BTreeSet::new();

    for line in text.lines() {
        if line.contains("Alternative name") {
            continue;
        }
        if line.contains("(video)") || line.contains("(none)") {
            if let Some(device) = first_quoted_text(line) {
                if !device.trim().is_empty() {
                    devices.insert(device.trim().to_string());
                }
            }
        }
    }

    devices.into_iter().collect()
}

fn first_quoted_text(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = line.get(start + 1..)?;
    let end = rest.find('"')?;
    rest.get(..end).map(str::to_string)
}

fn list_monitors_inner() -> Result<Vec<MonitorInfo>> {
    run_with_com(|| {
        let monitors = Monitor::all().context("Failed to enumerate monitors")?;
        let mut result = Vec::with_capacity(monitors.len());

        for monitor in monitors {
            let id = monitor.id().context("Failed to read monitor id")?;
            let name = monitor.name().unwrap_or_else(|_| format!("Monitor {id}"));
            let friendly_name = name.clone();
            result.push(MonitorInfo {
                id: id.to_string(),
                name,
                friendly_name,
                x: monitor.x().context("Failed to read monitor x position")?,
                y: monitor.y().context("Failed to read monitor y position")?,
                width: monitor.width().context("Failed to read monitor width")?,
                height: monitor.height().context("Failed to read monitor height")?,
                is_primary: monitor.is_primary().unwrap_or(false),
            });
        }

        Ok(result)
    })
}

fn newest_capture_path(state: &AppState) -> Result<PathBuf> {
    let recent = list_recent_captures_inner(state)?;
    recent
        .first()
        .map(|capture| PathBuf::from(&capture.path))
        .ok_or_else(|| anyhow!("No captures available for analysis."))
}

fn capture_kind(filename: &str) -> Option<&'static str> {
    if filename.starts_with("defEYE_webcam_") && filename.ends_with(".mp4") {
        Some("webcam")
    } else if filename.starts_with("defEYE_screen_") && filename.ends_with(".mp4") {
        Some("screen")
    } else if filename.starts_with("defEYE_current_") && filename.ends_with(".png") {
        Some("current")
    } else if filename.starts_with("defEYE_allmerged_") && filename.ends_with(".png") {
        Some("merged")
    } else if filename.starts_with("defEYE_timelapse_") && filename.ends_with(".png") {
        Some("timelapse")
    } else if filename.starts_with("defEYE_snapshot_") && filename.ends_with(".png") {
        Some("current")
    } else {
        None
    }
}

fn selected_monitor(settings: &Settings) -> Result<Monitor> {
    let monitors = Monitor::all().context("Failed to enumerate monitors")?;
    if !settings.primary_monitor_id.trim().is_empty() {
        for monitor in &monitors {
            if monitor
                .id()
                .map(|id| id.to_string() == settings.primary_monitor_id)
                .unwrap_or(false)
            {
                return Ok(monitor.clone());
            }
        }
    }

    for monitor in &monitors {
        if monitor.is_primary().unwrap_or(false) {
            return Ok(monitor.clone());
        }
    }
    monitors
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to capture screen: no monitor found."))
}

/// Build gdigrab input args for screen recording.
/// Returns a Vec of args to pass before `-i desktop` (e.g. `-video_size`, `WxH`, `-offset_x`, `X`, `-offset_y`, `Y`).
/// For all-monitors mode, returns empty (just uses `desktop` as-is).
fn build_gdigrab_input_args(settings: &Settings) -> Vec<String> {
    if settings.screen_capture_mode == ScreenCaptureMode::SpecificMonitor
        && !settings.screen_monitor_id.trim().is_empty()
    {
        let settings = settings.clone();
        let result = run_with_com(move || {
            let monitors = Monitor::all().context("Failed to enumerate monitors")?;
            for monitor in &monitors {
                if monitor
                    .id()
                    .map(|id| id.to_string() == settings.screen_monitor_id)
                    .unwrap_or(false)
                {
                    if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                        monitor.x(),
                        monitor.y(),
                        monitor.width(),
                        monitor.height(),
                    ) {
                        return Ok(Some(vec![
                            "-video_size".to_string(),
                            format!("{w}x{h}"),
                            "-offset_x".to_string(),
                            x.to_string(),
                            "-offset_y".to_string(),
                            y.to_string(),
                        ]));
                    }
                    break;
                }
            }
            Ok(None)
        });
        if let Ok(Some(args)) = result {
            return args;
        }
    }
    Vec::new()
}

fn load_or_create_settings(app: &AppHandle) -> Result<Settings> {
    let path = settings_path(app)?;
    if path.exists() {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        match serde_json::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                sanitize_settings(&mut settings)?;
                ensure_dir(&settings.output_dir)?;
                save_settings(app, &settings)?;
                return Ok(settings);
            }
            Err(e) => {
                eprintln!("[defEYE] Failed to parse settings ({}), recreating with defaults", e);
                let _ = fs::remove_file(&path);
            }
        }
    }

    let settings = Settings::default();
    ensure_dir(&settings.output_dir)?;
    save_settings(app, &settings)?;
    Ok(settings)
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<()> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(settings).context("Failed to serialize settings")?;
    fs::write(&tmp, json).with_context(|| format!("Failed to write {}", tmp.display()))?;
    replace_file(&tmp, &path)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_local_data_dir()
        .context("Failed to locate app local data directory")?
        .join(SETTINGS_FILE))
}

fn default_output_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("defEYE")
}

fn sanitize_settings(settings: &mut Settings) -> Result<()> {
    if !(18..=32).contains(&settings.crf) {
        bail!("CRF must be between 18 and 32.");
    }
    if !(5..=60).contains(&settings.screen_fps) {
        bail!("Screen FPS must be between 5 and 60.");
    }
    if !(18..=32).contains(&settings.screen_crf) {
        bail!("Screen CRF must be between 18 and 32.");
    }
    settings.output_dir = sanitize_user_path(settings.output_dir.clone())?;
    settings.camera_device = settings.camera_device.trim().to_string();
    settings.manual_camera_device = settings.manual_camera_device.trim().to_string();
    settings.primary_monitor_id = settings.primary_monitor_id.trim().to_string();
    settings.screen_monitor_id = settings.screen_monitor_id.trim().to_string();
    settings.webcam_audio_device = settings.webcam_audio_device.trim().to_string();
    settings.screen_audio_device = settings.screen_audio_device.trim().to_string();
    // Sanitize multi_camera_devices
    settings.multi_camera_devices = settings
        .multi_camera_devices
        .iter()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .collect();
    // Validate custom region
    if settings.custom_region.width == 0 {
        settings.custom_region.width = 1920;
    }
    if settings.custom_region.height == 0 {
        settings.custom_region.height = 1080;
    }
    if !(1..=100).contains(&settings.motion_sensitivity) {
        bail!("Motion sensitivity must be between 1 and 100.");
    }
    if settings.motion_cooldown_seconds == 0 {
        bail!("Motion cooldown must be at least 1 second.");
    }
    if !(0.0..=1.0).contains(&settings.watermark_opacity) {
        bail!("Watermark opacity must be between 0.0 and 1.0.");
    }
    if !(0.01..=1.0).contains(&settings.watermark_scale) {
        bail!("Watermark scale must be between 0.01 and 1.0.");
    }
    settings.watermark_image_path = settings.watermark_image_path.trim().to_string();
    if settings.watermark_image_enabled && settings.watermark_image_path.is_empty() {
        bail!("Image watermark is enabled but no image path is set.");
    }
    // Validate time-lapse interval
    if settings.timelapse_interval_seconds < TIMELAPSE_MIN_INTERVAL {
        settings.timelapse_interval_seconds = TIMELAPSE_MIN_INTERVAL;
    }
    // Migrate incompatible/renamed model names to qwen2.5vl:7b (works with Ollama 0.30+)
    if settings.ollama_model == "llama3.2-vision:11b" || settings.ollama_model == "qwen2.5-vl:7b" {
        eprintln!("[defEYE] Migrating ollama_model from '{}' to 'qwen2.5vl:7b' (compatibility fix)", settings.ollama_model);
        settings.ollama_model = "qwen2.5vl:7b".to_string();
    }
    // Sanitize voice settings
    settings.voice_audio_device = settings.voice_audio_device.trim().to_string();
    settings.voice_wake_word = settings.voice_wake_word.trim().to_string();
    settings.voice_model_path = settings.voice_model_path.trim().replace("\\\\", "\\").to_string();
    if !(0.0..=1.0).contains(&settings.voice_confidence_threshold) {
        settings.voice_confidence_threshold = 0.65;
    }
    Ok(())
}

fn sanitize_user_path(path: PathBuf) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("Output directory cannot be empty.");
    }
    if has_parent_dir(&path) {
        bail!("Paths containing '..' are not allowed.");
    }
    Ok(path)
}

fn sanitize_existing_path(path: PathBuf) -> Result<PathBuf> {
    let path = sanitize_user_path(path)?;
    if !path.exists() {
        bail!("Path does not exist: {}", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("Failed to resolve {}", path.display()))
}

fn ensure_inside_dir(path: &Path, dir: &Path) -> Result<()> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("Failed to resolve {}", dir.display()))?;
    let path = path
        .canonicalize()
        .with_context(|| format!("Failed to resolve {}", path.display()))?;
    if !path.starts_with(&dir) {
        bail!("Refusing to operate outside output directory.");
    }
    Ok(())
}

fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("Failed to create {}", path.display()))
}

fn write_png_atomic(path: &Path, image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension("png.tmp");
    {
        let file =
            File::create(&tmp).with_context(|| format!("Failed to create {}", tmp.display()))?;
        let mut writer = BufWriter::new(file);
        image
            .write_to(&mut writer, ImageFormat::Png)
            .with_context(|| format!("Failed to encode {}", path.display()))?;
    }
    replace_file(&tmp, path)
}

fn replace_file(tmp: &Path, final_path: &Path) -> Result<()> {
    if final_path.exists() {
        fs::remove_file(final_path)
            .with_context(|| format!("Failed to replace {}", final_path.display()))?;
    }
    fs::rename(tmp, final_path)
        .with_context(|| format!("Failed to finalize {}", final_path.display()))
}

fn effective_camera_device(settings: &Settings) -> String {
    if !settings.manual_camera_device.trim().is_empty() {
        settings.manual_camera_device.trim().to_string()
    } else {
        settings.camera_device.trim().to_string()
    }
}

fn normalize_dshow_input(device: &str) -> String {
    let trimmed = device.trim();
    if trimmed.starts_with("video=") {
        trimmed.to_string()
    } else {
        format!("video={trimmed}")
    }
}

fn sanitize_recording_kind(kind: &str) -> String {
    match kind.trim().to_ascii_lowercase().as_str() {
        "webcam" => "webcam".to_string(),
        "screen" => "screen".to_string(),
        "mixed" => "mixed".to_string(),
        "multi" => "multi".to_string(),
        _ => "idle".to_string(),
    }
}

fn timestamp() -> String {
    Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

fn format_system_time(time: SystemTime) -> String {
    let datetime: DateTime<Local> = time.into();
    datetime.to_rfc3339()
}

fn status_payload(state: &AppState) -> StatusPayload {
    let webcam_active = state.recording_child.lock().is_some();
    let screen_active = state.screen_recording_child.lock().is_some();
    let multi_active = !state.multi_recording_children.lock().is_empty();
    let finalizing = state.finalizing_count.load(Ordering::SeqCst) > 0;
    let is_recording = webcam_active || screen_active || multi_active;
    let recording_kind = if !is_recording && finalizing {
        "finalizing".to_string()
    } else if !is_recording {
        "idle".to_string()
    } else if screen_active && (webcam_active || multi_active) {
        "mixed".to_string()
    } else if screen_active {
        "screen".to_string()
    } else {
        "webcam".to_string()
    };

    StatusPayload {
        is_recording,
        recording_kind,
        webcam_active,
        screen_active,
        multi_active,
        finalizing,
    }
}

fn emit_status(app: &AppHandle, state: &AppState) {
    let payload = status_payload(state);
    let _ = app.emit("status-updated", payload);
}

fn emit_motion_status(app: &AppHandle, state: &AppState) {
    let payload = motion_status_payload(state);
    let _ = app.emit("motion-status-updated", payload);
}

fn motion_status_payload(state: &AppState) -> MotionStatusPayload {
    let settings = state.settings.lock();
    let motion_active = state.motion_active.load(Ordering::SeqCst);
    let last_detection = state
        .motion_last_detection
        .lock()
        .map(|time| {
            let datetime: DateTime<Local> =
                DateTime::<Local>::from(time);
            datetime.to_rfc3339()
        });
    MotionStatusPayload {
        motion_mode_enabled: settings.motion_mode_enabled,
        motion_active,
        last_detection,
    }
}

fn toggle_motion_mode_inner(app: &AppHandle, state: &AppState) -> Result<bool> {
    let mut settings = state.settings.lock().clone();
    settings.motion_mode_enabled = !settings.motion_mode_enabled;
    let now_enabled = settings.motion_mode_enabled;

    save_settings(app, &settings)?;
    *state.settings.lock() = settings;

    if now_enabled {
        start_motion_detection_inner(app.clone(), state)?;
    } else {
        stop_motion_detection_inner(state);
    }
    emit_motion_status(app, state);
    Ok(now_enabled)
}

fn start_motion_detection_inner(app: AppHandle, state: &AppState) -> Result<()> {
    if state.motion_child.lock().is_some() {
        return Ok(());
    }

    let settings = state.settings.lock().clone();
    let device = effective_camera_device(&settings);
    if device.trim().is_empty() {
        bail!("Cannot start motion detection: no camera device selected.");
    }

    // Stop camera preview if active — both use the same DirectShow device
    if state.camera_preview_active.load(Ordering::SeqCst) {
        eprintln!("[defEYE] Stopping camera preview for motion detection");
        state.camera_preview_active.store(false, Ordering::SeqCst);
        let path = state.preview_path.lock().take();
        if let Some(p) = path {
            let _ = fs::remove_file(p);
        }
    }

    let scene_threshold = scene_threshold_from_sensitivity(settings.motion_sensitivity);
    eprintln!(
        "[defEYE] Motion detection starting: device={device}, sensitivity={}, threshold={scene_threshold:.4}",
        settings.motion_sensitivity
    );

    let input = normalize_dshow_input(&device);
    let filter = format!("select='gt(scene,{scene_threshold})',showinfo");

    let mut command = ffmpeg_command();
    command.arg("-y");
    command.args(["-f", "dshow", "-rtbufsize", "100M", "-framerate", "5", "-i", &input]);
    command.args(["-filter:v", &filter]);
    command.args(["-f", "null", "-"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow!("Failed to start motion detection: ffmpeg not found in PATH. Please install ffmpeg.")
        } else {
            anyhow!("Failed to start motion detection: {error}")
        }
    })?;

    // Store child and mark motion active — return immediately after spawning monitor threads
    *state.motion_child.lock() = Some(child);
    state.motion_active.store(true, Ordering::SeqCst);
    emit_motion_status(&app, state);

    // Spawn the auto-stop monitor thread (handles cooldown-based recording stop)
    spawn_motion_auto_stop_thread(
        app.clone(),
        state.motion_active.clone(),
        state.motion_last_detection.clone(),
        settings.motion_min_record_seconds,
        settings.motion_post_record_seconds,
    );

    // Spawn the ffmpeg output reader thread (parses scene detection output, triggers recording)
    spawn_motion_reader_thread(
        app.clone(),
        state.motion_child.clone(),
        state.motion_active.clone(),
        state.motion_last_detection.clone(),
        settings,
    );

    Ok(())
}

/// Auto-stop monitor thread: periodically checks if recording should be stopped
/// after motion ceases, respecting min_record_seconds and post_record_seconds.
fn spawn_motion_auto_stop_thread(
    app: AppHandle,
    motion_active: Arc<AtomicBool>,
    motion_last_detection: Arc<Mutex<Option<SystemTime>>>,
    min_record_secs: u32,
    post_record_secs: u32,
) {
    thread::spawn(move || {
        let mut was_recording = false;
        let mut record_start: Option<SystemTime> = None;

        while motion_active.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));

            let state = app.state::<AppState>();
            let is_recording = state.recording_child.lock().is_some()
                || state.screen_recording_child.lock().is_some()
                || !state.multi_recording_children.lock().is_empty();

            if is_recording && record_start.is_none() {
                record_start = Some(SystemTime::now());
                was_recording = true;
            }

            if !is_recording {
                was_recording = false;
                record_start = None;
                continue;
            }

            if !was_recording || record_start.is_none() {
                continue;
            }

            let Some(start_time) = record_start else {
                continue;
            };
            let now = SystemTime::now();
            let elapsed_since_start = now
                .duration_since(start_time)
                .unwrap_or(Duration::ZERO);

            // Don't stop before min_record_seconds
            if elapsed_since_start < Duration::from_secs(u64::from(min_record_secs)) {
                continue;
            }

            let last_motion = *motion_last_detection.lock();
            if let Some(last_time) = last_motion {
                let elapsed_since_motion = now
                    .duration_since(last_time)
                    .unwrap_or(Duration::ZERO);

                if elapsed_since_motion >= Duration::from_secs(u64::from(post_record_secs)) {
                    eprintln!(
                        "[defEYE] Motion stopped {}s ago (post_record={}s), stopping recording",
                        elapsed_since_motion.as_secs(),
                        post_record_secs
                    );
                    let _ = stop_all_recording_inner(&app, state.inner());
                    refresh_recording_state_from_children(state.inner(), &app);
                    was_recording = false;
                    record_start = None;

                    // Restart motion detection if it's enabled but no longer active
                    // (it was paused to free the camera for recording)
                    let settings = state.settings.lock().clone();
                    if settings.motion_mode_enabled && !state.motion_active.load(Ordering::SeqCst) {
                        eprintln!("[defEYE] Resuming motion detection after recording stopped");
                        let _ = start_motion_detection_inner(app.clone(), state.inner());
                    }
                }
            }
        }
    });
}

/// ffmpeg output reader thread: reads stderr/stdout from the motion detection
/// ffmpeg process, parses scene change events, and triggers recording on motion.
/// Also handles graceful cleanup when the ffmpeg process exits or motion is stopped.
fn spawn_motion_reader_thread(
    app: AppHandle,
    motion_child: Arc<Mutex<Option<Child>>>,
    motion_active: Arc<AtomicBool>,
    motion_last_detection: Arc<Mutex<Option<SystemTime>>>,
    settings: Settings,
) {
    let cooldown_secs = settings.motion_cooldown_seconds;
    let auto_record = settings.auto_record_on_motion;
    let triggers_screen = settings.motion_triggers_screen;
    let output_dir = settings.output_dir.clone();
    let settings_snapshot = settings;

    thread::spawn(move || {
        // Take stderr/stdout from the child while holding the lock briefly
        let (stderr, stdout) = {
            let mut child_guard = motion_child.lock();
            match child_guard.as_mut() {
                Some(c) => (c.stderr.take(), c.stdout.take()),
                None => return,
            }
        };

        let reader: Box<dyn BufRead> = match stderr {
            Some(stream) => Box::new(BufReader::new(stream)),
            None => match stdout {
                Some(stream) => Box::new(BufReader::new(stream)),
                None => return,
            },
        };

        for line_result in reader.lines() {
            if !motion_active.load(Ordering::SeqCst) {
                break;
            }
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            // Log ffmpeg errors so we can diagnose startup failures
            if line.contains("Could not") || line.contains("error") || line.contains("Error") || line.contains("No such") {
                eprintln!("[defEYE] motion ffmpeg: {line}");
            }

            if line.contains("showinfo") && line.contains("pts_time:") {
                let now = SystemTime::now();
                let should_trigger = {
                    let last = *motion_last_detection.lock();
                    match last {
                        Some(last_time) => {
                            now.duration_since(last_time)
                                .map(|elapsed| elapsed > Duration::from_secs(u64::from(cooldown_secs)))
                                .unwrap_or(true)
                        }
                        None => true,
                    }
                };

                if should_trigger {
                    *motion_last_detection.lock() = Some(now);
                    let ts = timestamp();
                    let _ = append_motion_log(&output_dir, &ts);
                    let _ = app.emit(
                        "motion-detected",
                        MotionDetectedPayload {
                            timestamp: ts.clone(),
                            message: "Motion detected".to_string(),
                        },
                    );

                    let state = app.state::<AppState>();

                    if auto_record && state.recording_child.lock().is_none() && state.multi_recording_children.lock().is_empty() {
                        // Stop motion detection ffmpeg to free the DirectShow camera device
                        // so the recording ffmpeg can access it. Restart after recording stops.
                        let motion_was_active = state.motion_active.load(Ordering::SeqCst);
                        if motion_was_active {
                            eprintln!("[defEYE] Pausing motion detection to free camera for recording");
                            stop_motion_detection_inner(state.inner());
                            // Give the DirectShow device time to fully release
                            thread::sleep(Duration::from_millis(500));
                        }

                        let record_result = if settings_snapshot.multi_camera_mode == MultiCameraMode::Multi && !settings_snapshot.multi_camera_devices.is_empty() {
                            start_multi_recording_inner(
                                &app, state.inner(), &settings_snapshot,
                            )
                        } else {
                            start_recording_with_settings(
                                &app, state.inner(), &settings_snapshot,
                            ).map(|_| String::new())
                        };

                        if let Err(e) = &record_result {
                            eprintln!("[defEYE] Motion-triggered recording failed to start: {e}");
                            // Recording failed — restart motion detection immediately
                            if motion_was_active {
                                let _ = start_motion_detection_inner(app.clone(), state.inner());
                            }
                        }

                        refresh_recording_state_from_children(
                            state.inner(),
                            &app,
                        );
                    }

                    if triggers_screen && state.screen_recording_child.lock().is_none() {
                        let _ = start_screen_recording_with_settings(
                            &app, state.inner(), &settings_snapshot,
                        );
                        refresh_recording_state_from_children(
                            state.inner(),
                            &app,
                        );
                    }
                }
            }
        }

        // ffmpeg exited or motion_active was set to false.
        // If the child is still registered, own it and stop/wait outside all locks.
        motion_active.store(false, Ordering::SeqCst);
        let lingering_child = motion_child.lock().take();
        if let Some(mut child) = lingering_child {
            graceful_stop_child(&mut child);
        }
        let state = app.state::<AppState>();
        emit_motion_status(&app, state.inner());
    });
}

fn stop_motion_detection_inner(state: &AppState) {
    state.motion_active.store(false, Ordering::SeqCst);
    let child = state.motion_child.lock().take();
    if let Some(child) = child {
        thread::spawn(move || stop_child_owned(child));
    }
}

fn stop_motion_detection_blocking(state: &AppState) {
    state.motion_active.store(false, Ordering::SeqCst);
    let child = state.motion_child.lock().take();
    if let Some(mut child) = child {
        graceful_stop_child(&mut child);
    }
}

fn restart_motion_detection_async(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<AppState>();
        stop_motion_detection_blocking(state.inner());
        if let Err(error) = start_motion_detection_inner(app.clone(), state.inner()) {
            let _ = app.emit("defeye-error", error.to_string());
        }
    });
}

fn scene_threshold_from_sensitivity(sensitivity: u8) -> f64 {
    let clamped = sensitivity.clamp(1, 100) as f64;
    0.003 + (1.0 - clamped / 100.0) * 0.047
}

fn append_motion_log(output_dir: &Path, timestamp: &str) -> Result<()> {
    ensure_dir(output_dir)?;
    let log_path = output_dir.join("motion_log.txt");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Failed to open {}", log_path.display()))?;
    writeln!(file, "{timestamp} - Motion detected").context("Failed to write motion log")?;
    Ok(())
}

fn start_recording_with_settings(
    app: &AppHandle,
    state: &AppState,
    settings: &Settings,
) -> Result<()> {
    if state.recording_child.lock().is_some() {
        return Ok(());
    }
    let device = effective_camera_device(settings);
    if device.trim().is_empty() {
        bail!("Cannot start recording: no camera device selected.");
    }
    ensure_dir(&settings.output_dir)?;

    let output = settings
        .output_dir
        .join(format!("defEYE_webcam_{}.mp4", timestamp()));
    let input = build_webcam_dshow_input(settings, &device);
    let has_audio = settings.webcam_audio_enabled && !settings.webcam_audio_device.trim().is_empty();
    let wm = build_watermark_filter(settings, 1, if has_audio { Some(0) } else { None });

    let mut builder = FfmpegCommandBuilder::new()
        .dshow_input(&input, "100M")
        .watermark(wm, has_audio);

    if settings.embed_metadata {
        builder = builder.metadata("defEYE Capture", &format!("defEYE webcam capture {}", timestamp()));
    }

    let mut child = builder
        .x264_encoding(&settings.crf.to_string())
        .output(&output)
        .stdio_recording()
        .spawn_with_error("Failed to start recording")?;

    verify_ffmpeg_started(&mut child, "ffmpeg failed to start")?;

    *state.recording_child.lock() = Some(child);
    *state.recording_output_path.lock() = Some(output.clone());
    let _ = app.emit(
        "file-created",
        FileCreatedPayload { path: output.to_string_lossy().to_string() },
    );
    Ok(())
}

fn start_screen_recording_with_settings(
    app: &AppHandle,
    state: &AppState,
    settings: &Settings,
) -> Result<()> {
    if state.screen_recording_child.lock().is_some() {
        return Ok(());
    }
    ensure_dir(&settings.output_dir)?;

    let output = settings
        .output_dir
        .join(format!("defEYE_screen_{}.mp4", timestamp()));
    let fps = settings.screen_fps.to_string();
    let (_, eff_screen_crf) = apply_recording_preset(settings.recording_preset, settings.crf, settings.screen_crf);
    let crf = eff_screen_crf.to_string();

    let gdigrab_args = build_gdigrab_input_args(settings);

    let has_audio = settings.screen_audio_enabled && !settings.screen_audio_device.trim().is_empty();
    let wm_idx = if has_audio { 2 } else { 1 };
    let audio_idx = if has_audio { Some(1) } else { None };
    let wm = build_watermark_filter(settings, wm_idx, audio_idx);

    let mut builder = FfmpegCommandBuilder::new()
        .gdigrab_input(&fps, "100M", &gdigrab_args);

    if has_audio {
        builder = builder.dshow_audio_input(settings.screen_audio_device.trim());
    }

    builder = builder.watermark(wm, has_audio);

    if settings.embed_metadata {
        builder = builder.metadata("defEYE Screen Capture", &format!("defEYE screen capture {}", timestamp()));
    }

    let mut child = builder
        .x264_encoding(&crf)
        .output(&output)
        .stdio_recording()
        .spawn_with_error("Failed to start screen recording")?;

    verify_ffmpeg_started(&mut child, "ffmpeg failed to start")?;

    *state.screen_recording_child.lock() = Some(child);
    *state.screen_recording_output_path.lock() = Some(output.clone());
    let _ = app.emit(
        "file-created",
        FileCreatedPayload { path: output.to_string_lossy().to_string() },
    );
    Ok(())
}

fn refresh_recording_state_from_children(
    state: &AppState,
    app: &AppHandle,
) {
    refresh_recording_state(state);
    emit_status(app, state);
}

fn emit_file_created(app: &AppHandle, path: &Path) {
    let _ = app.emit(
        "file-created",
        FileCreatedPayload {
            path: path.to_string_lossy().to_string(),
        },
    );
}

// ---------------------------------------------------------------------------
// Audio device listing
// ---------------------------------------------------------------------------

fn list_audio_devices_inner() -> Result<Vec<AudioDevice>> {
    let text = run_dshow_list_devices()?;
    let devices = parse_dshow_audio_devices(&text);
    eprintln!("[defEYE] parsed {} audio devices", devices.len());
    Ok(devices)
}

fn parse_dshow_audio_devices(text: &str) -> Vec<AudioDevice> {
    let mut devices = Vec::new();

    for line in text.lines() {
        if line.contains("Alternative name") {
            continue;
        }
        if line.contains("(audio)") {
            if let Some(name) = first_quoted_text(line) {
                if !name.trim().is_empty() {
                    devices.push(AudioDevice {
                        name: name.trim().to_string(),
                        index: devices.len(),
                    });
                }
            }
        }
    }

    devices
}

// ---------------------------------------------------------------------------
// Build dshow input string with optional audio
// ---------------------------------------------------------------------------

fn build_webcam_dshow_input(settings: &Settings, video_device: &str) -> String {
    let trimmed_video = video_device.trim();
    if trimmed_video.contains(":audio=") || trimmed_video.starts_with("audio=") {
        return trimmed_video.to_string();
    }

    let audio_device = settings.webcam_audio_device.trim();
    if settings.webcam_audio_enabled && !audio_device.is_empty() {
        let video_part = if trimmed_video.starts_with("video=") {
            trimmed_video.to_string()
        } else {
            format!("video={trimmed_video}")
        };
        format!("{video_part}:audio={audio_device}")
    } else {
        normalize_dshow_input(trimmed_video)
    }
}

// ---------------------------------------------------------------------------
// Multi-camera recording
// ---------------------------------------------------------------------------

fn start_multi_recording_inner(
    app: &AppHandle,
    state: &AppState,
    settings: &Settings,
) -> Result<String> {
    if !state.multi_recording_children.lock().is_empty() {
        return Ok("Multi-camera recording is already active".to_string());
    }
    if state.recording_child.lock().is_some() {
        bail!("Cannot start multi-camera recording while a single webcam recording is active. Stop it first.");
    }

    let devices = &settings.multi_camera_devices;
    if devices.is_empty() {
        bail!("No multi-camera devices selected.");
    }

    ensure_dir(&settings.output_dir)?;
    let mut children = Vec::new();
    let mut outputs = Vec::new();
    let mut output_paths = Vec::new();

    for (i, device_name) in devices.iter().enumerate() {
        if device_name.trim().is_empty() {
            continue;
        }
        let output = settings.output_dir.join(format!(
            "defEYE_webcam_cam{}_{}.mp4",
            i + 1,
            timestamp()
        ));
        let input = if i == 0 {
            build_webcam_dshow_input(settings, device_name)
        } else {
            normalize_dshow_input(device_name)
        };
        eprintln!("[defEYE] multi-cam{} dshow input: {:?}", i + 1, input);
        let has_audio = i == 0 && settings.webcam_audio_enabled && !settings.webcam_audio_device.trim().is_empty();
        let wm = build_watermark_filter(settings, 1, if has_audio { Some(0) } else { None });
        let (eff_crf, _) = apply_recording_preset(settings.recording_preset, settings.crf, settings.screen_crf);

        let mut builder = FfmpegCommandBuilder::new()
            .dshow_input(&input, "100M")
            .watermark(wm, has_audio);

        if settings.embed_metadata {
            builder = builder.metadata("defEYE Multi-Camera Capture", &format!("defEYE cam{} capture {}", i + 1, timestamp()));
        }

        let spawn_result = builder
            .x264_encoding(&eff_crf.to_string())
            .output(&output)
            .stdio_recording()
            .spawn_with_error(&format!("Failed to start multi-camera recording for cam{} (device: {device_name})", i + 1));

        let mut child = match spawn_result {
            Ok(child) => child,
            Err(error) => {
                stop_children_blocking(&mut children);
                if error.to_string().contains("not found in PATH") {
                    bail!("Failed to start multi-camera recording: ffmpeg not found in PATH.");
                }
                bail!("{error}");
            }
        };

        if let Err(error) = verify_ffmpeg_started(&mut child, &format!("ffmpeg failed to start for cam{} (device: {device_name})", i + 1)) {
            eprintln!("[defEYE] cam{} failed to start, cleaning up: {error}", i + 1);
            graceful_stop_child(&mut child);
            stop_children_blocking(&mut children);
            return Err(error);
        }

        eprintln!("[defEYE] cam{} started successfully, output: {}", i + 1, output.display());
        output_paths.push(output.clone());
        outputs.push(output.to_string_lossy().to_string());
        children.push(child);
    }

    if children.is_empty() {
        bail!("No valid camera devices found for multi-camera recording.");
    }

    *state.multi_recording_children.lock() = children;
    *state.multi_recording_output_paths.lock() = output_paths.clone();
    refresh_recording_state(state);
    emit_status(app, state);

    // Emit file-created events for each output so the UI knows about them
    for path in &output_paths {
        emit_file_created(app, path);
    }

    Ok(outputs.join(", "))
}

// ---------------------------------------------------------------------------
// Camera cycling (quick-switch mode)
// ---------------------------------------------------------------------------

fn cycle_camera_inner(app: &AppHandle, state: &AppState, direction: i32) -> Result<String> {
    let settings = state.settings.lock().clone();
    if settings.multi_camera_mode != MultiCameraMode::QuickSwitch {
        bail!("Camera cycling is only available in quick-switch mode.");
    }
    let devices = &settings.multi_camera_devices;
    if devices.is_empty() {
        bail!("No multi-camera devices configured for cycling.");
    }

    let current = state.active_camera_index.load(Ordering::SeqCst).min(devices.len() - 1);
    let len = devices.len();
    let next = if direction > 0 {
        (current + 1) % len
    } else if current == 0 {
        len - 1
    } else {
        current - 1
    };
    state.active_camera_index.store(next, Ordering::SeqCst);

    let mut new_settings = settings.clone();
    new_settings.camera_device = devices[next].clone();
    save_settings(app, &new_settings)?;
    *state.settings.lock() = new_settings.clone();

    let device_name = devices[next].clone();
    let was_recording = state.recording_child.lock().is_some();
    if was_recording {
        let old_path = state.recording_output_path.lock().take();
        let old_child = state.recording_child.lock().take();
        begin_finalizing(state);
        refresh_recording_state(state);
        emit_status(app, state);

        let app_handle = app.clone();
        thread::spawn(move || {
            if let Some(mut child) = old_child {
                graceful_stop_child(&mut child);
            }

            let state = app_handle.state::<AppState>();
            if let Err(error) = start_recording_with_settings(&app_handle, state.inner(), &new_settings) {
                let _ = app_handle.emit("defeye-error", error.to_string());
            }
            refresh_recording_state_from_children(state.inner(), &app_handle);

            if let Some(ref path) = old_path {
                post_process_capture(&app_handle, &settings, path);
            }
            finish_finalizing(&app_handle);
        });
    }

    let _ = app.emit("camera-cycled", device_name.clone());
    Ok(device_name)
}

// ---------------------------------------------------------------------------
// Screen monitor switching (mid-recording)
// ---------------------------------------------------------------------------

fn switch_screen_monitor_inner(
    app: &AppHandle,
    state: &AppState,
    monitor_id: &str,
) -> Result<String> {
    let mut settings = state.settings.lock().clone();

    // Update settings with new monitor selection
    if monitor_id.is_empty() {
        settings.screen_capture_mode = ScreenCaptureMode::AllMonitors;
        settings.screen_monitor_id = String::new();
    } else {
        settings.screen_capture_mode = ScreenCaptureMode::SpecificMonitor;
        settings.screen_monitor_id = monitor_id.to_string();
    }
    save_settings(app, &settings)?;
    *state.settings.lock() = settings.clone();

    let was_recording = state.screen_recording_child.lock().is_some();

    if was_recording {
        // Stop current recording
        let old_path = state.screen_recording_output_path.lock().take();
        let old_child = state.screen_recording_child.lock().take();
        begin_finalizing(state);
        refresh_recording_state(state);
        emit_status(app, state);

        let app_handle = app.clone();
        let old_settings = settings.clone();
        thread::spawn(move || {
            if let Some(mut child) = old_child {
                graceful_stop_child(&mut child);
            }

            let state = app_handle.state::<AppState>();
            if let Err(error) =
                start_screen_recording_with_settings(&app_handle, state.inner(), &settings)
            {
                let _ = app_handle.emit("defeye-error", error.to_string());
            }
            refresh_recording_state_from_children(state.inner(), &app_handle);

            if let Some(ref path) = old_path {
                post_process_capture(&app_handle, &old_settings, path);
            }
            finish_finalizing(&app_handle);
        });

        let label = if monitor_id.is_empty() {
            "all monitors".to_string()
        } else {
            monitor_id.to_string()
        };
        let _ = app.emit("screen-monitor-switched", &label);
        Ok(format!("Switched screen recording to {label}"))
    } else {
        let label = if monitor_id.is_empty() {
            "all monitors".to_string()
        } else {
            monitor_id.to_string()
        };
        let _ = app.emit("screen-monitor-switched", &label);
        Ok(format!("Screen target set to {label}"))
    }
}

// ---------------------------------------------------------------------------
// Region cropping
// ---------------------------------------------------------------------------

fn crop_image(
    image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    region: &CustomRegion,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let img_w = image.width();
    let img_h = image.height();

    let x = region.x.max(0) as u32;
    let y = region.y.max(0) as u32;
    let w = region.width.min(img_w.saturating_sub(x));
    let h = region.height.min(img_h.saturating_sub(y));

    if w == 0 || h == 0 {
        return image.clone();
    }

    image::imageops::crop_imm(image, x, y, w, h).to_image()
}

// ---------------------------------------------------------------------------
// Evidence hardening: watermark, metadata, integrity
// ---------------------------------------------------------------------------

/// Returns a Windows font path suitable for ffmpeg drawtext.
fn windows_font_path() -> &'static str {
    // Try common Windows fonts; fallback to arial
    let candidates = [
        "C:/Windows/Fonts/consola.ttf",
        "C:/Windows/Fonts/arial.ttf",
        "C:/Windows/Fonts/segoeui.ttf",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path;
        }
    }
    // Fallback — ffmpeg will error if it can't find it, but at least we tried
    "C:/Windows/Fonts/arial.ttf"
}

/// Compute x/y expressions for watermark positioning in ffmpeg filters.
fn watermark_position_expr(pos: WatermarkPosition, custom_x: i32, custom_y: i32) -> (String, String) {
    match pos {
        WatermarkPosition::TopLeft => ("10".to_string(), "10".to_string()),
        WatermarkPosition::TopRight => ("W-w-10".to_string(), "10".to_string()),
        WatermarkPosition::BottomLeft => ("10".to_string(), "H-h-10".to_string()),
        WatermarkPosition::BottomRight => ("W-w-10".to_string(), "H-h-10".to_string()),
        WatermarkPosition::Center => ("(W-w)/2".to_string(), "(H-h)/2".to_string()),
        WatermarkPosition::Custom => (custom_x.to_string(), custom_y.to_string()),
    }
}

/// Build the `-vf` filter string for text watermark (drawtext).
/// Uses fontfile to fix the watermark not appearing on Windows.
fn build_text_watermark_vf(settings: &Settings) -> Option<String> {
    if !settings.watermark_enabled {
        return None;
    }
    let font = windows_font_path();
    let escaped_font = font.replace(':', "\\:");
    let ts = timestamp().replace("'", "\\'");
    let (x_expr, y_expr) = watermark_position_expr(settings.watermark_position, settings.watermark_x, settings.watermark_y);
    let alpha = settings.watermark_opacity.clamp(0.0, 1.0);
    Some(format!(
        "drawtext=fontfile='{escaped_font}':text='defEYE {ts}':x={x_expr}:y={y_expr}:fontsize=14:fontcolor=white@{alpha}"
    ))
}

/// Combine text watermark vf and image watermark into a single filter chain for ffmpeg.
pub enum WatermarkFilter {
    None,
    TextVf(String),
    FilterComplex {
        extra_inputs: Vec<String>,
        filter: String,
        map_args: Vec<String>,
    },
}

/// Build the appropriate watermark filter for a video recording.
/// `wm_input_index` is the ffmpeg input index where the watermark image will be added.
/// `audio_input_index` is Some(idx) if there's an audio stream to map, None otherwise.
fn build_watermark_filter(settings: &Settings, wm_input_index: usize, audio_input_index: Option<usize>) -> WatermarkFilter {
    let has_text = settings.watermark_enabled;
    let has_image = settings.watermark_image_enabled && !settings.watermark_image_path.trim().is_empty();

    if has_image {
        let img_path = settings.watermark_image_path.trim();
        let scale = settings.watermark_scale.clamp(0.01, 1.0);
        let (x_expr, y_expr) = watermark_position_expr(settings.watermark_position, settings.watermark_x, settings.watermark_y);
        let alpha = settings.watermark_opacity.clamp(0.0, 1.0);

        let extra_inputs = vec!["-i".to_string(), img_path.to_string()];

        // Build filter: scale watermark, overlay on main video, optionally add drawtext
        let mut filter = format!(
            "[{wm}:v]scale=iw*{scale}:-1,format=rgba,colorchannelmixer=aa={alpha}[wm_scaled];[0:v][wm_scaled]overlay={x_expr}:{y_expr}",
            wm = wm_input_index
        );

        if has_text {
            let font = windows_font_path();
            let escaped_font = font.replace(':', "\\:");
            let ts = timestamp().replace("'", "\\'");
            let (tx, ty) = watermark_position_expr(settings.watermark_position, settings.watermark_x, settings.watermark_y);
            let text_alpha = settings.watermark_opacity.clamp(0.0, 1.0);
            filter.push_str(&format!(
                ",drawtext=fontfile='{escaped_font}':text='defEYE {ts}':x={tx}:y={ty}:fontsize=14:fontcolor=white@{text_alpha}"
            ));
        }

        filter.push_str("[vout]");

        // Build map args: always map video, optionally map audio
        let mut map_args = vec!["-map".to_string(), "[vout]".to_string()];
        if let Some(audio_idx) = audio_input_index {
            map_args.push("-map".to_string());
            map_args.push(format!("{audio_idx}:a"));
        }

        WatermarkFilter::FilterComplex { extra_inputs, filter, map_args }
    } else if has_text {
        let vf = build_text_watermark_vf(settings).unwrap_or_default();
        WatermarkFilter::TextVf(vf)
    } else {
        WatermarkFilter::None
    }
}

fn watermark_png(image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>) {
    let width = image.width();
    let height = image.height();
    let ts = timestamp();
    let text = format!("defEYE {}", ts);

    // Simple pixel-based watermark: draw text in bottom-right corner
    // Draw a semi-transparent bar + text using basic pixel manipulation
    let bar_height = 20u32;
    let bar_y = height.saturating_sub(bar_height);
    let text_bytes = text.as_bytes();
    let text_width = (text_bytes.len() as u32) * 7; // approx 7px per char
    let text_x = width.saturating_sub(text_width + 10);

    // Draw semi-transparent bar
    for py in bar_y..height {
        for px in 0..width {
            let pixel = image.get_pixel_mut(px, py);
            pixel[0] = (pixel[0] / 2).min(80);
            pixel[1] = (pixel[1] / 2).min(80);
            pixel[2] = (pixel[2] / 2).min(80);
            pixel[3] = 200;
        }
    }

    // Draw text as light pixels using a simple 5x7 font approximation
    for (i, &byte) in text_bytes.iter().enumerate() {
        let char_x = text_x + (i as u32) * 7;
        let char_y = bar_y + 6;
        for fy in 0..7u32 {
            for fx in 0..5u32 {
                let bit = (byte >> (fy % 8)) & (1 << (fx % 8));
                if bit != 0 && char_x + fx < width && char_y + fy < height {
                    let pixel = image.get_pixel_mut(char_x + fx, char_y + fy);
                    pixel[0] = 200;
                    pixel[1] = 200;
                    pixel[2] = 200;
                    pixel[3] = 220;
                }
            }
        }
    }
}

/// Apply both text and image watermarks to a PNG image.
/// Text watermark uses the existing pixel-based watermark_png.
/// Image watermark loads the image file, scales it, applies opacity, and composites it.
fn apply_png_watermark(image: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, settings: &Settings) {
    // Apply image watermark first (so text appears on top if both enabled)
    if settings.watermark_image_enabled && !settings.watermark_image_path.trim().is_empty() {
        let img_path = Path::new(&settings.watermark_image_path);
        if let Ok(watermark_img) = image::open(img_path) {
            let watermark_rgba = watermark_img.to_rgba8();
            let img_w = image.width();
            let img_h = image.height();

            // Scale watermark relative to image width, preserving aspect ratio
            let target_w = ((img_w as f32) * settings.watermark_scale.clamp(0.01, 1.0)).round() as u32;
            let target_w = target_w.max(1);
            let target_h = if watermark_rgba.width() > 0 {
                ((target_w as f32) * (watermark_rgba.height() as f32) / (watermark_rgba.width() as f32)).round() as u32
            } else {
                1
            };
            let target_h = target_h.max(1);
            let scaled = image::imageops::resize(
                &watermark_rgba,
                target_w,
                target_h,
                image::imageops::FilterType::Lanczos3,
            );

            let wm_w = scaled.width();
            let wm_h = scaled.height();

            // Compute position
            let (x, y) = match settings.watermark_position {
                WatermarkPosition::TopLeft => (10u32, 10u32),
                WatermarkPosition::TopRight => (img_w.saturating_sub(wm_w).saturating_sub(10), 10),
                WatermarkPosition::BottomLeft => (10, img_h.saturating_sub(wm_h).saturating_sub(10)),
                WatermarkPosition::BottomRight => (
                    img_w.saturating_sub(wm_w).saturating_sub(10),
                    img_h.saturating_sub(wm_h).saturating_sub(10),
                ),
                WatermarkPosition::Center => (
                    img_w.saturating_sub(wm_w) / 2,
                    img_h.saturating_sub(wm_h) / 2,
                ),
                WatermarkPosition::Custom => {
                    let cx = settings.watermark_x.max(0) as u32;
                    let cy = settings.watermark_y.max(0) as u32;
                    (cx, cy)
                }
            };

            let alpha = settings.watermark_opacity.clamp(0.0, 1.0);

            // Composite watermark with alpha blending
            for py in 0..wm_h {
                for px in 0..wm_w {
                    let dst_x = x + px;
                    let dst_y = y + py;
                    if dst_x >= img_w || dst_y >= img_h {
                        continue;
                    }
                    let wm_pixel = scaled.get_pixel(px, py);
                    let wm_alpha = (wm_pixel[3] as f32 / 255.0) * alpha;
                    if wm_alpha <= 0.0 {
                        continue;
                    }
                    let dst_pixel = image.get_pixel_mut(dst_x, dst_y);
                    dst_pixel[0] = ((wm_pixel[0] as f32 * wm_alpha) + (dst_pixel[0] as f32 * (1.0 - wm_alpha))) as u8;
                    dst_pixel[1] = ((wm_pixel[1] as f32 * wm_alpha) + (dst_pixel[1] as f32 * (1.0 - wm_alpha))) as u8;
                    dst_pixel[2] = ((wm_pixel[2] as f32 * wm_alpha) + (dst_pixel[2] as f32 * (1.0 - wm_alpha))) as u8;
                    // Keep destination alpha
                }
            }
        } else {
            eprintln!("[defEYE] Warning: could not open watermark image: {}", settings.watermark_image_path);
        }
    }

    // Apply text watermark on top
    if settings.watermark_enabled {
        watermark_png(image);
    }
}

fn write_watermark_sidecar(file_path: &Path) -> Result<()> {
    let sidecar_path = file_path.with_extension("watermark.json");
    let sidecar = serde_json::json!({
        "file": file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "watermarked": true,
        "timestamp": timestamp(),
        "method": if file_path.extension().and_then(|e| e.to_str()) == Some("mp4") {
            "ffmpeg_drawtext"
        } else {
            "png_overlay"
        },
    });
    let json = serde_json::to_string_pretty(&sidecar)?;
    fs::write(&sidecar_path, json)
        .with_context(|| format!("Failed to write watermark sidecar {}", sidecar_path.display()))?;
    Ok(())
}

fn write_integrity_sidecar(file_path: &Path) -> Result<()> {
    let hash = compute_sha256(file_path)?;
    let sidecar_path = file_path.with_extension("sha256.json");
    let sidecar = serde_json::json!({
        "file": file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "sha256": hash,
        "timestamp": timestamp(),
    });
    let json = serde_json::to_string_pretty(&sidecar)?;
    fs::write(&sidecar_path, json)
        .with_context(|| format!("Failed to write integrity sidecar {}", sidecar_path.display()))?;
    Ok(())
}

fn write_metadata_sidecar(file_path: &Path, kind: &str) -> Result<()> {
    let sidecar_path = file_path.with_extension("meta.json");
    let sidecar = serde_json::json!({
        "file": file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "kind": kind,
        "captured": timestamp(),
        "tool": "defEYE",
        "version": "1.1.0",
    });
    let json = serde_json::to_string_pretty(&sidecar)?;
    fs::write(&sidecar_path, json)
        .with_context(|| format!("Failed to write metadata sidecar {}", sidecar_path.display()))?;
    Ok(())
}

fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)
            .with_context(|| format!("Failed to read {} for hashing", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_integrity_inner(path: &str) -> Result<IntegrityResult> {
    let file_path = sanitize_existing_path(PathBuf::from(path))?;
    let sidecar_path = file_path.with_extension("sha256.json");

    if !sidecar_path.exists() {
        // No sidecar — compute actual hash but can't verify
        let actual = compute_sha256(&file_path).ok();
        return Ok(IntegrityResult {
            verified: false,
            stored_hash: None,
            actual_hash: actual,
            message: "No integrity sidecar found. File was not created with integrity check enabled.".to_string(),
        });
    }

    let sidecar_text = fs::read_to_string(&sidecar_path)
        .with_context(|| format!("Failed to read sidecar {}", sidecar_path.display()))?;
    let sidecar: serde_json::Value = serde_json::from_str(&sidecar_text)
        .context("Failed to parse integrity sidecar JSON")?;
    let stored_hash = sidecar
        .get("sha256")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let actual_hash = compute_sha256(&file_path).ok();

    let verified = match (&stored_hash, &actual_hash) {
        (Some(stored), Some(actual)) => stored == actual,
        _ => false,
    };

    let message = if verified {
        "Integrity verified: SHA256 hash matches.".to_string()
    } else {
        "Integrity check FAILED: SHA256 hash does not match.".to_string()
    };

    Ok(IntegrityResult {
        verified,
        stored_hash,
        actual_hash,
        message,
    })
}

// ---------------------------------------------------------------------------
// Post-processing after recording stops
// ---------------------------------------------------------------------------

fn post_process_capture(app: &AppHandle, settings: &Settings, path: &Path) {
    // Always emit file-created if the file exists, even if post-processing has issues
    match post_process_capture_inner(settings, path) {
        Ok(()) => {
            emit_file_created(app, path);
        }
        Err(error) => {
            // File exists but some post-processing step failed — still notify the UI
            if path.exists() {
                emit_file_created(app, path);
            }
            let _ = app.emit(
                "defeye-error",
                format!("Post-processing warning for {}: {error}", path.display()),
            );
        }
    }

    // Auto-analysis hook: run defEYE analysis in background if enabled
    if settings.ollama_enabled && settings.auto_analysis_on_capture {
        let app_handle = app.clone();
        let path = path.to_path_buf();
        thread::spawn(move || {
            let default_prompt = "Describe any people, movement, or anomalies in this security capture. Be factual, concise, and alert on anything unusual.";
            let state = app_handle.state::<AppState>();
            let _ = run_defeye_analysis_inner(
                &app_handle,
                state.inner(),
                Some(path.to_string_lossy().to_string()),
                default_prompt.to_string(),
            );
        });
    }
}

fn post_process_capture_inner(settings: &Settings, path: &Path) -> Result<()> {
    wait_for_file_ready(path)?;

    // Sidecars and thumbnails are non-fatal — the MP4 is the important part
    if settings.watermark_enabled || settings.watermark_image_enabled {
        if let Err(e) = write_watermark_sidecar(path) {
            eprintln!("[defEYE] Warning: watermark sidecar failed: {e}");
        }
    }

    if settings.integrity_check {
        if let Err(e) = write_integrity_sidecar(path) {
            eprintln!("[defEYE] Warning: integrity sidecar failed: {e}");
        }
    }

    if settings.embed_metadata {
        let kind = capture_kind_from_path(path).unwrap_or("unknown");
        if let Err(e) = write_metadata_sidecar(path, kind) {
            eprintln!("[defEYE] Warning: metadata sidecar failed: {e}");
        }
    }

    if let Err(e) = generate_thumbnail(path, &settings.output_dir) {
        eprintln!("[defEYE] Warning: thumbnail generation failed: {e}");
    }

    Ok(())
}

fn capture_kind_from_path(path: &Path) -> Option<&'static str> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    capture_kind(filename)
}

fn wait_for_file_ready(path: &Path) -> Result<()> {
    let deadline = Instant::now() + FILE_FINALIZE_TIMEOUT;
    let mut previous_len = None;
    let mut stable_ticks = 0u8;

    while Instant::now() < deadline {
        if let Ok(metadata) = fs::metadata(path) {
            let len = metadata.len();
            if len > 0 && previous_len == Some(len) {
                stable_ticks = stable_ticks.saturating_add(1);
                if stable_ticks >= 2 {
                    return Ok(());
                }
            } else {
                stable_ticks = 0;
                previous_len = Some(len);
            }
        }
        thread::sleep(Duration::from_millis(150));
    }

    if !path.exists() {
        bail!("Final capture file was not created: {}", path.display());
    }
    let len = fs::metadata(path)
        .with_context(|| format!("Failed to read finalized capture {}", path.display()))?
        .len();
    if len == 0 {
        bail!("Final capture file is empty: {}", path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Thumbnail generation
// ---------------------------------------------------------------------------

fn generate_thumbnail(file_path: &Path, output_dir: &Path) -> Result<()> {
    let thumbs_dir = output_dir.join("thumbnails");
    ensure_dir(&thumbs_dir)?;

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let thumb_path = thumbs_dir.join(format!("{filename}.png"));

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if ext == "png" {
        // Generate thumbnail from PNG using image crate
        let img = image::open(file_path)
            .with_context(|| format!("Failed to open image for thumbnail: {}", file_path.display()))?;
        let thumb_width = 160u32;
        let thumb_height = if img.width() > 0 {
            ((thumb_width as f32) * (img.height() as f32) / (img.width() as f32)).round() as u32
        } else {
            120
        };
        let thumb_height = thumb_height.max(1);
        let thumb = img.resize(thumb_width, thumb_height, image::imageops::FilterType::Lanczos3);
        thumb.save(&thumb_path)
            .with_context(|| format!("Failed to save thumbnail: {}", thumb_path.display()))?;
    } else if ext == "mp4" {
        let file_arg = file_path.to_string_lossy().to_string();
        let thumb_arg = thumb_path.to_string_lossy().to_string();

        // Try seeking to 1s first; if that fails (short recording), try first frame
        let attempts: &[&[&str]] = &[
            &["-y", "-ss", "00:00:01", "-i", &file_arg, "-frames:v", "1", "-vf", "scale=160:-1", "-f", "image2", &thumb_arg],
            &["-y", "-i", &file_arg, "-frames:v", "1", "-vf", "scale=160:-1", "-f", "image2", &thumb_arg],
        ];

        let mut success = false;
        for args in attempts {
            let status = ffmpeg_command()
                .args(*args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if status.map(|s| s.success()).unwrap_or(false) {
                success = true;
                break;
            }
        }

        if !success {
            bail!("ffmpeg could not extract a thumbnail frame from {}", file_path.display());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Graceful child stop helper
// ---------------------------------------------------------------------------

fn graceful_stop_child(child: &mut Child) {
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }

    let deadline = Instant::now() + FFMPEG_STOP_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn stop_child_owned(mut child: Child) {
    graceful_stop_child(&mut child);
}

fn stop_children_blocking(children: &mut Vec<Child>) {
    for mut child in children.drain(..) {
        graceful_stop_child(&mut child);
    }
}

fn ffmpeg_command() -> Command {
    let mut command = Command::new("ffmpeg");
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Builder for constructing ffmpeg commands with common patterns.
/// Encapsulates argument ordering, watermark application, codec settings,
/// metadata, stdio configuration, and spawn error handling.
struct FfmpegCommandBuilder {
    command: Command,
}

#[allow(dead_code)]
impl FfmpegCommandBuilder {
    /// Create a new builder with `ffmpeg` as the program and `-y` as the first arg.
    fn new() -> Self {
        let mut command = ffmpeg_command();
        command.arg("-y");
        Self { command }
    }

    /// Add a DirectShow video input: `-f dshow -rtbufsize <buf> -i <input>`
    fn dshow_input(mut self, input: &str, rtbufsize: &str) -> Self {
        self.command.args(["-f", "dshow", "-rtbufsize", rtbufsize, "-i", input]);
        self
    }

    /// Add a DirectShow video input with framerate: `-f dshow -rtbufsize <buf> -framerate <fps> -i <input>`
    fn dshow_input_with_framerate(mut self, input: &str, rtbufsize: &str, framerate: &str) -> Self {
        self.command.args(["-f", "dshow", "-rtbufsize", rtbufsize, "-framerate", framerate, "-i", input]);
        self
    }

    /// Add a gdigrab desktop input: `-f gdigrab -framerate <fps> -rtbufsize <buf> [extra_args...] -i desktop`
    fn gdigrab_input(mut self, fps: &str, rtbufsize: &str, extra_args: &[String]) -> Self {
        self.command.args(["-f", "gdigrab", "-framerate", fps, "-rtbufsize", rtbufsize]);
        for arg in extra_args {
            self.command.arg(arg);
        }
        self.command.arg("-i").arg("desktop");
        self
    }

    /// Add a secondary DirectShow audio input: `-f dshow -i audio=<device>`
    fn dshow_audio_input(mut self, audio_device: &str) -> Self {
        let audio_input = format!("audio={audio_device}");
        self.command.args(["-f", "dshow", "-i", &audio_input]);
        self
    }

    /// Apply a watermark filter and audio codec flags.
    /// `wm` is the WatermarkFilter from `build_watermark_filter`.
    /// `has_audio` controls whether `-acodec aac` or `-an` is added.
    fn watermark(mut self, wm: WatermarkFilter, has_audio: bool) -> Self {
        match wm {
            WatermarkFilter::TextVf(vf) => {
                self.command.args(["-vf", &vf]);
                self = self.audio_codec(has_audio);
            }
            WatermarkFilter::FilterComplex { extra_inputs, filter, map_args } => {
                for arg in extra_inputs {
                    self.command.arg(arg);
                }
                self.command.args(["-filter_complex", &filter]);
                for arg in map_args {
                    self.command.arg(arg);
                }
                self = self.audio_codec(has_audio);
            }
            WatermarkFilter::None => {
                self = self.audio_codec(has_audio);
            }
        }
        self
    }

    /// Add audio codec flags: `-acodec aac` if has_audio, otherwise `-an`.
    fn audio_codec(mut self, has_audio: bool) -> Self {
        if has_audio {
            self.command.args(["-acodec", "aac"]);
        } else {
            self.command.args(["-an"]);
        }
        self
    }

    /// Add metadata: `-metadata title=<title> -metadata comment=<comment>`
    fn metadata(mut self, title: &str, comment: &str) -> Self {
        self.command.args(["-metadata", &format!("title={title}")]);
        self.command.args(["-metadata", &format!("comment={comment}")]);
        self
    }

    /// Add standard video encoding args: `-vcodec libx264 -preset veryfast -crf <crf> -pix_fmt yuv420p -movflags +faststart`
    fn x264_encoding(mut self, crf: &str) -> Self {
        self.command.args([
            "-vcodec", "libx264", "-preset", "veryfast",
            "-crf", crf,
            "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        ]);
        self
    }

    /// Add a raw arg to the command.
    fn arg(mut self, arg: &str) -> Self {
        self.command.arg(arg);
        self
    }

    /// Add raw args to the command.
    fn args(mut self, args: &[&str]) -> Self {
        self.command.args(args);
        self
    }

    /// Set the output file path.
    fn output(mut self, path: &Path) -> Self {
        self.command.arg(path);
        self
    }

    /// Configure stdio for recording: stdin piped, stdout null, stderr piped.
    fn stdio_recording(mut self) -> Self {
        self.command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        self
    }

    /// Configure stdio for one-shot commands: all null.
    fn stdio_silent(mut self) -> Self {
        self.command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        self
    }

    /// Configure stdio for commands needing piped stderr.
    fn stdio_stderr_piped(mut self) -> Self {
        self.command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        self
    }

    /// Configure stdio for motion detection: stdin piped, stdout piped, stderr piped.
    fn stdio_all_piped(mut self) -> Self {
        self.command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        self
    }

    /// Spawn the ffmpeg process with a NotFound-aware error message.
    fn spawn_with_error(mut self, error_label: &str) -> Result<Child> {
        self.command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!("{error_label}: ffmpeg not found in PATH. Please install ffmpeg.")
            } else {
                anyhow!("{error_label}: {error}")
            }
        })
    }

    /// Run the command to completion (blocking) with silent stdio, returning success bool.
    fn run_silent(mut self) -> std::io::Result<std::process::ExitStatus> {
        self.command.status()
    }

    /// Run the command to completion (blocking) with piped stderr, returning output.
    fn run_with_stderr(mut self) -> std::io::Result<std::process::Output> {
        self.command.output()
    }

    /// Consume the builder and return the underlying Command.
    fn build(self) -> Command {
        self.command
    }
}

fn ffprobe_command() -> Command {
    let mut command = Command::new("ffprobe");
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn get_video_duration(path: &Path) -> Option<f64> {
    let output = ffprobe_command()
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse::<f64>().ok()
}

fn drain_child_stderr(child: &mut Child) {
    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buffer = [0u8; 8192];
            while reader.read(&mut buffer).unwrap_or(0) > 0 {}
        });
    }
}

fn verify_ffmpeg_started(child: &mut Child, context_label: &str) -> Result<()> {
    thread::sleep(FFMPEG_STARTUP_GRACE);
    match child.try_wait() {
        Ok(Some(status)) => {
            let stderr_msg = child
                .stderr
                .take()
                .map(|stream| {
                    let mut buf = String::new();
                    let _ = BufReader::new(stream).read_to_string(&mut buf);
                    buf
                })
                .unwrap_or_default();
            let last_line = stderr_msg
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("unknown ffmpeg error");
            eprintln!("[defEYE] ffmpeg full stderr:\n{}", stderr_msg);
            bail!("{context_label}: {last_line} (exit status: {status})");
        }
        Ok(None) => {
            drain_child_stderr(child);
            Ok(())
        }
        Err(error) => bail!("{context_label}: failed to inspect ffmpeg process: {error}"),
    }
}

fn to_user_error(error: anyhow::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_kind_webcam() {
        assert_eq!(capture_kind("defEYE_webcam_2024-01-01_12-00-00.mp4"), Some("webcam"));
    }

    #[test]
    fn test_capture_kind_screen() {
        assert_eq!(capture_kind("defEYE_screen_2024-01-01_12-00-00.mp4"), Some("screen"));
    }

    #[test]
    fn test_capture_kind_current() {
        assert_eq!(capture_kind("defEYE_current_2024-01-01_12-00-00.png"), Some("current"));
    }

    #[test]
    fn test_capture_kind_merged() {
        assert_eq!(capture_kind("defEYE_allmerged_2024-01-01_12-00-00.png"), Some("merged"));
    }

    #[test]
    fn test_capture_kind_timelapse() {
        assert_eq!(capture_kind("defEYE_timelapse_2024-01-01_12-00-00.png"), Some("timelapse"));
        assert_eq!(capture_kind("defEYE_timelapse_screen_2024-01-01_12-00-00.png"), Some("timelapse"));
        assert_eq!(capture_kind("defEYE_timelapse_webcam_2024-01-01_12-00-00.png"), Some("timelapse"));
    }

    #[test]
    fn test_capture_kind_snapshot() {
        assert_eq!(capture_kind("defEYE_snapshot_2024-01-01_12-00-00.png"), Some("current"));
    }

    #[test]
    fn test_capture_kind_from_path_webcam() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/defEYE_webcam_2024-01-01_12-00-00.mp4")), Some("webcam"));
    }

    #[test]
    fn test_capture_kind_from_path_screen() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/defEYE_screen_2024-01-01_12-00-00.mp4")), Some("screen"));
    }

    #[test]
    fn test_capture_kind_from_path_current() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/defEYE_current_2024-01-01_12-00-00.png")), Some("current"));
    }

    #[test]
    fn test_capture_kind_from_path_merged() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/defEYE_allmerged_2024-01-01_12-00-00.png")), Some("merged"));
    }

    #[test]
    fn test_capture_kind_from_path_timelapse() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/timelapse/defEYE_timelapse_2024-01-01_12-00-00.png")), Some("timelapse"));
    }

    #[test]
    fn test_capture_kind_from_path_unknown() {
        assert_eq!(capture_kind_from_path(Path::new("C:/captures/random_file.txt")), None);
    }

    #[test]
    fn test_scene_threshold_high_sensitivity() {
        let threshold = scene_threshold_from_sensitivity(100);
        assert!((threshold - 0.003).abs() < 0.001, "expected ~0.003, got {threshold}");
    }

    #[test]
    fn test_scene_threshold_low_sensitivity() {
        let threshold = scene_threshold_from_sensitivity(1);
        assert!((threshold - 0.05).abs() < 0.001, "expected ~0.05, got {threshold}");
    }

    #[test]
    fn test_scene_threshold_clamped() {
        let threshold = scene_threshold_from_sensitivity(0);
        assert!((threshold - 0.05).abs() < 0.001, "expected clamp to 1, got {threshold}");
    }

    #[test]
    fn test_parse_dshow_video_devices() {
        let text = "[dshow] \"USB Camera\" (video)\n[dshow] \"Microphone\" (audio)\n[dshow] Alternative name \"@device_cm_{1234}\"";
        let devices = parse_dshow_video_devices(text);
        assert_eq!(devices, vec!["USB Camera"]);
    }

    #[test]
    fn test_parse_dshow_audio_devices() {
        let text = "[dshow] \"USB Camera\" (video)\n[dshow] \"Microphone\" (audio)\n[dshow] \"Speakers\" (audio)";
        let devices = parse_dshow_audio_devices(text);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Microphone");
        assert_eq!(devices[1].name, "Speakers");
    }

    #[test]
    fn test_first_quoted_text() {
        assert_eq!(first_quoted_text("hello \"world\" foo"), Some("world".to_string()));
        assert_eq!(first_quoted_text("no quotes"), None);
    }

    #[test]
    fn test_sanitize_recording_kind() {
        assert_eq!(sanitize_recording_kind("webcam"), "webcam");
        assert_eq!(sanitize_recording_kind("SCREEN"), "screen");
        assert_eq!(sanitize_recording_kind("mixed"), "mixed");
        assert_eq!(sanitize_recording_kind("  Multi  "), "multi");
        assert_eq!(sanitize_recording_kind("unknown"), "idle");
    }

    #[test]
    fn test_normalize_dshow_input() {
        assert_eq!(normalize_dshow_input("USB Camera"), "video=USB Camera");
        assert_eq!(normalize_dshow_input("video=USB Camera"), "video=USB Camera");
    }

    #[test]
    fn test_has_parent_dir() {
        assert!(has_parent_dir(Path::new("../etc/passwd")));
        assert!(!has_parent_dir(Path::new("C:/Users/foo/bar")));
    }

    #[test]
    fn test_apply_recording_preset() {
        assert_eq!(apply_recording_preset(RecordingPreset::Ultra, 25, 25), (18, 18));
        assert_eq!(apply_recording_preset(RecordingPreset::High, 25, 25), (20, 20));
        assert_eq!(apply_recording_preset(RecordingPreset::Medium, 23, 24), (23, 24));
        assert_eq!(apply_recording_preset(RecordingPreset::Low, 25, 25), (28, 28));
        assert_eq!(apply_recording_preset(RecordingPreset::Custom, 23, 24), (23, 24));
    }

    #[test]
    fn test_similarity_ratio_identical() {
        assert!((similarity_ratio("hello", "hello") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_similarity_ratio_completely_different() {
        assert!((similarity_ratio("abc", "xyz") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_similarity_ratio_non_ascii() {
        // "café" has 4 chars but 5 bytes. Normalization must use char count.
        let ratio = similarity_ratio("café", "cafe");
        // Levenshtein distance is 1 (é vs e), max char count is 4.
        assert!((ratio - 0.75).abs() < 0.01, "expected ~0.75, got {ratio}");
    }

    #[test]
    fn test_similarity_ratio_empty() {
        assert!((similarity_ratio("", "") - 1.0).abs() < f32::EPSILON);
        assert!((similarity_ratio("", "abc") - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_timelapse_target_str() {
        assert_eq!(timelapse_target_str(TimeLapseTarget::Screen), "screen");
        assert_eq!(timelapse_target_str(TimeLapseTarget::Webcam), "webcam");
        assert_eq!(timelapse_target_str(TimeLapseTarget::Both), "both");
    }

    #[test]
    fn test_levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }
}

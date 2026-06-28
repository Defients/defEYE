# defEYE

defEYE v1.2 is a local-only Tauri v2 desktop sentinel for Windows 10/11. It records webcam video and full screen video through `ffmpeg`, captures selected-primary or merged multi-monitor screenshots through `xcap`, and stays hidden unless summoned.

Tagline: **The Unblinking Sentinel**

## Privacy

Strictly personal use. defEYE has no telemetry, no cloud integration, no tray plugin, and no external calls. The Analysis tab only reports local capture metadata unless you later wire a local vision runtime yourself.

## Requirements

- Windows 10/11
- Rust stable
- Node.js + pnpm
- `ffmpeg` installed and available in `PATH`

Install ffmpeg on Windows with one of:

```bash
winget install Gyan.FFmpeg
choco install ffmpeg
```

Verify from a new terminal:

```bash
ffmpeg -version
```

## Build & Run

```bash
pnpm install
pnpm tauri dev
pnpm tauri build
```

`pnpm tauri dev` starts hidden by design. Use the settings hotkey below to show the settings window. Release builds are emitted under:

```text
src-tauri/target/release/defEYE.exe
```

## Hardcoded Hotkeys

- `Shift+ArrowUp` starts webcam recording.
- `Shift+ArrowDown` stops webcam recording and finalizes the file.
- `Ctrl+ArrowUp` starts screen recording (desktop via gdigrab).
- `Ctrl+ArrowDown` stops any active recording (webcam or screen).
- `Ctrl+ArrowLeft` captures the current/configured primary screen.
- `Ctrl+ArrowRight` captures all screens and merges them into one virtual-desktop PNG.
- `Ctrl+Shift+ArrowDown` shows or focuses the settings window.
- `Ctrl+Shift+ArrowUp` toggles Sentinel Motion Mode on/off instantly.
- `Ctrl+Shift+ArrowLeft` cycles to the previous camera (quick-switch mode).
- `Ctrl+Shift+ArrowRight` cycles to the next camera (quick-switch mode).
- `Ctrl+Alt+ArrowDown` toggles Stealth Mode (instantly hide/show all defEYE windows).
- `Ctrl+Alt+ArrowRight` toggles Time-Lapse capture on/off.

## HUD

defEYE creates a separate `hud` window during setup:

- Transparent, borderless, always on top, skipped from the taskbar.
- About 118x28 px, click-through where the OS/runtime supports it.
- Default position: top-right corner of the primary monitor.
- Shows a subtle green idle dot or red recording dot with minimal status text.
- Position can be changed in settings.

## Window Controls

The main settings window uses `decorations(false)` — no native title bar. Instead:

- **Drag Handle**: A 4-directional cross icon in the header lets you click and drag to move the window.
- **Minimize Button**: Minimizes the settings window to the taskbar.
- **Hide Button**: Hides the settings window to background (process keeps running).
- Use `Ctrl+Shift+ArrowDown` to show/focus the settings window again via hotkey.

## Files

Default output directory:

```text
%USERPROFILE%\Documents\defEYE\
```

Config file:

```text
app_local_data_dir()/defeye_settings.json
```

Capture names:

```text
defEYE_webcam_YYYY-MM-DD_HH-MM-SS.mp4
defEYE_webcam_cam1_YYYY-MM-DD_HH-MM-SS.mp4  (multi-camera)
defEYE_webcam_cam2_YYYY-MM-DD_HH-MM-SS.mp4  (multi-camera)
defEYE_screen_YYYY-MM-DD_HH-MM-SS.mp4
defEYE_current_YYYY-MM-DD_HH-MM-SS.png
defEYE_allmerged_YYYY-MM-DD_HH-MM-SS.png
defEYE_timelapse_YYYY-MM-DD_HH-MM-SS.png      (time-lapse captures)
defEYE_snapshot_YYYY-MM-DD_HH-MM-SS.png       (extracted video snapshots)
motion_log.txt
thumbnails/                                 (auto-generated thumbnails)
*.sha256.json                               (integrity sidecars)
*.meta.json                                 (metadata sidecars)
*.note.json                                 (annotation sidecars)
*.watermark.json                            (watermark sidecars)
```

## Camera Selection

The Camera tab can refresh devices by running:

```text
ffmpeg -list_devices true -f dshow -i dummy
```

On Windows, defEYE parses DirectShow video device names into the camera dropdown. The manual ffmpeg device field overrides the dropdown when filled.

## Camera Preview

The Camera tab includes an **Activate** button that starts a live camera preview without recording. This runs a background ffmpeg process that continuously captures JPEG frames to a temporary file, which the UI polls and displays.

- **Activate / Deactivate**: Toggles the camera preview on and off.
- The preview runs independently from recording — you can preview, then start recording separately.
- The preview automatically stops when the settings window is closed or the app exits.

## Audio Control

defEYE v1.1 adds per-source audio device selection for both webcam and screen recording.

### Webcam Audio

- **Webcam Audio Enabled** (default: on): Toggle audio capture for webcam recordings.
- **Audio Device**: Select a specific DirectShow audio input device (microphone). When selected, ffmpeg uses the combined `video=...:audio=...` dshow input syntax.
- The legacy **Include Audio** toggle serves as a fallback AAC flag when no specific audio device is chosen.

### Screen Audio

- **Screen Audio Enabled** (default: off): Toggle audio capture for screen recordings.
- **Audio Device**: Select a specific DirectShow audio input device for screen recording audio.
- When enabled, ffmpeg adds a separate dshow audio input alongside the gdigrab video input.

Audio devices are listed via the same ffmpeg `-list_devices` command, parsing lines tagged with `(audio)`.

## Multi-Camera Support

defEYE v1.1 supports multiple cameras with two modes:

### Modes

- **Single** (default): Standard single-camera recording.
- **Multi — Simultaneous**: Records from all selected cameras at once, each writing to a separate file (`defEYE_webcam_cam1_*.mp4`, `defEYE_webcam_cam2_*.mp4`, etc.).
- **Quick-Switch**: Records from one camera at a time. Use hotkeys to cycle between cameras without stopping the recording — the current recording is stopped and a new one starts with the next camera.

### Camera Selection

In Multi or Quick-Switch mode, a checklist of detected cameras appears. Select which cameras to include. In Quick-Switch mode, the active camera is updated in settings when cycled.

### Hotkeys (Quick-Switch mode only)

- `Ctrl+Shift+ArrowLeft`: Cycle to the previous camera.
- `Ctrl+Shift+ArrowRight`: Cycle to the next camera.

If a recording is active when cycling, it is stopped and restarted with the new camera automatically.

## Region Selection for Screenshots

defEYE v1.1 adds screenshot region control:

- **Full** (default): Capture the full virtual desktop (all monitors merged).
- **Primary**: Capture only the primary monitor.
- **Custom**: Capture a specific rectangular region defined by X, Y, Width, and Height values.

When Custom mode is selected, the captured image is cropped to the specified region using the `image` crate's crop functionality before saving.

## Evidence Hardening

defEYE v1.1 introduces evidence hardening features for chain-of-custody integrity:

### Watermarking

- **Watermark Enabled** (default: off): Embeds a "defEYE {timestamp}" watermark on captures.
- For video recordings: Uses ffmpeg's `drawtext` filter during encoding.
- For PNG screenshots: Pixel-level watermark is applied post-capture using the `image` crate.

### Metadata Embedding

- **Embed Metadata** (default: off): Adds metadata to capture files.
- For video: ffmpeg `-metadata` flags embed title and comment tags in the MP4 container.
- For all files: A `*.meta.json` sidecar file is written with capture metadata (file name, kind, timestamp, tool, version).

### Integrity Check (SHA256)

- **Integrity Check** (default: off): Computes a SHA256 hash of the capture file and stores it in a `*.sha256.json` sidecar.
- The sidecar contains the file name, SHA256 hash, and timestamp.
- The Captures tab includes a **Verify** button that recomputes the hash and compares it against the stored sidecar to detect tampering.

## Captures Tab Enhancements

- **Thumbnails**: Auto-generated preview thumbnails for both PNG and MP4 files. PNG thumbnails are generated using the `image` crate; video thumbnails are extracted via ffmpeg at the 1-second mark.
- **Integrity Badges**: Shield icons indicate which captures have integrity sidecars or watermarks.
- **Verify Button**: Run on-demand SHA256 integrity verification directly from the captures table.
- **Increased Limit**: Up to 100 recent captures are listed (was 50).
- **Annotation Badges**: Sticky note icons indicate which captures have annotation sidecars.
- **Snapshot Extraction**: Scissors button on video captures opens a dialog to extract a frame at a specified timestamp as a PNG.
- **Note Editor**: Sticky note button opens a modal to add, edit, or clear text annotations for any capture. Annotations are stored as `*.note.json` sidecar files.

## Stealth Mode

defEYE v1.2 introduces Stealth Mode — a one-touch toggle to instantly hide all defEYE windows (settings, HUD, region selector) from the screen.

- **Hotkey**: `Ctrl+Alt+ArrowDown` toggles stealth mode on/off.
- **Button**: A stealth icon in the header also toggles it.
- When engaged, all defEYE windows are hidden. Disengaging restores them.
- The state is tracked with an `AtomicBool` and emitted to the frontend via the `stealth-toggled` event.

## Disk Sentinel

defEYE v1.2 monitors disk space and automatically stops recordings when free space drops below a configurable threshold.

- **Threshold** (default: 1000 MB): Set the minimum free disk space in MB. When free space falls below this, all active recordings are stopped gracefully.
- **Check Interval**: 10 seconds — a background thread queries free disk space via the Windows `GetDiskFreeSpaceExW` API.
- **Warning Indicator**: The header shows a red disk space indicator when below threshold.
- **Pre-recording Check**: Before starting any recording, disk space is checked. If below threshold, the recording is refused with an error message.

## Sentinel Watchdog

defEYE v1.2 includes a watchdog that detects ffmpeg process crashes and automatically recovers recordings.

- **Enabled** (default: on): Toggle watchdog monitoring.
- **Check Interval**: 5 seconds — a background thread checks if recording child processes are still alive.
- If a webcam or screen recording process has exited unexpectedly, the watchdog automatically restarts the recording with the same settings.
- Recovery events are logged and emitted to the frontend.

## Capture Vault — Retention Policy

defEYE v1.2 can automatically purge old captures based on configurable age, count, and size limits.

- **Retention Enabled** (default: off): Master toggle for auto-cleanup.
- **Max Age** (days, default: 30, 0 = unlimited): Captures older than this are deleted.
- **Max Count** (default: 0 = unlimited): When exceeded, the oldest captures are deleted first.
- **Max Size** (GB, default: 0 = unlimited): When total capture size exceeds this, oldest captures are deleted first.
- **Check Interval**: 300 seconds (5 minutes) — a background sentinel thread runs the purge.
- **Purge Now**: A manual button triggers immediate cleanup and reports how many files were removed.
- All constraints are applied simultaneously — the most restrictive wins.

## Time-Lapse Capture

defEYE v1.2 supports interval-based time-lapse capture for both screen and webcam.

- **Interval** (seconds, min 5, default: 0 = disabled): Set the time between each capture.
- **Target**: Choose `screen` (screenshot via xcap) or `webcam` (single frame via ffmpeg dshow).
- **Hotkey**: `Ctrl+Alt+ArrowRight` toggles time-lapse on/off.
- Files are named `defEYE_timelapse_YYYY-MM-DD_HH-MM-SS.png`.
- Each frame is post-processed (thumbnail, watermark, integrity, metadata) like any other capture.
- Status is emitted via the `timelapse-status` event and shown in the UI.

## Snapshot Extraction

defEYE v1.2 can extract a still frame from any video recording at a specified timestamp.

- Available from the Captures tab — a scissors button on video captures opens a dialog.
- Enter a timestamp in seconds and click Extract.
- ffmpeg seeks to the specified time and extracts a single frame as PNG.
- Files are named `defEYE_snapshot_YYYY-MM-DD_HH-MM-SS.png`.
- The extracted snapshot is post-processed like any other capture.

## Capture Annotations

defEYE v1.2 supports adding text notes to any capture via sidecar JSON files.

- Available from the Captures tab — a sticky note button opens the annotation editor.
- Notes are stored as `*.note.json` sidecar files alongside the capture.
- A sticky note badge in the captures table indicates which captures have annotations.
- Setting an empty note clears the sidecar file.

## Enhanced HUD — Recording Duration

defEYE v1.2 shows a live recording duration timer in the header.

- The timer updates every second while a recording is active.
- Displays elapsed time in `MM:SS` format.
- Recording start times are tracked with `Mutex<Option<SystemTime>>` for both webcam and screen recordings.
- The `get_recording_duration` command returns elapsed seconds (or null if not recording).

## Screen Recording

defEYE records the full desktop (primary monitor) as MP4 video using ffmpeg's `gdigrab` input on Windows:

```text
ffmpeg -f gdigrab -framerate {fps} -i desktop -vcodec libx264 -preset veryfast -crf {crf} -pix_fmt yuv420p -movflags +faststart output.mp4
```

- **FPS** (default 15, range 5–60): Controls capture framerate. Lower FPS reduces CPU usage and file size.
- **CRF** (default 23, range 18–32): Controls quality. Lower values = higher quality / larger files.
- Both are adjustable via sliders in the Camera tab under the "Screen Recording" section.
- Screen recording is independent from webcam recording — both can run simultaneously (not recommended).
- `Ctrl+ArrowUp` starts screen recording; `Ctrl+ArrowDown` stops any active recording.
- Files are named `defEYE_screen_YYYY-MM-DD_HH-MM-SS.mp4`.
- The HUD shows "REC SCREEN" while screen recording is active.
- With audio enabled, ffmpeg adds dshow audio inputs and AAC encoding.

## Sentinel Motion Mode

defEYE includes an optional smart-trigger system that monitors the webcam feed for scene changes and can automatically start recording when motion is detected.

### How It Works

When Motion Mode is enabled, defEYE spawns a background ffmpeg process that reads from the selected webcam and applies the `select='gt(scene,THRESHOLD)'` filter with `showinfo` output. The background thread parses ffmpeg's stderr for `showinfo` lines indicating a scene change above the computed threshold.

### Settings

- **Motion Mode Enabled** (default: off): Master toggle for motion detection.
- **Sensitivity** (1–100, default: 50): Maps inversely to ffmpeg's scene threshold. Higher sensitivity = lower threshold = more detections. Range maps to scene threshold 0.01–0.30.
- **Cooldown** (seconds, default: 30): Minimum time between motion triggers to avoid spam.
- **Auto-record on Motion** (default: on): Automatically starts webcam recording when motion is detected (if not already recording).
- **Min Record Seconds** (default: 5): Minimum recording duration once motion triggers. The auto-stop monitor will not stop the recording before this time elapses.
- **Post-Record Seconds** (default: 15): How long to keep recording after the last motion detection event. Once no motion has been detected for this duration (and the minimum record time has elapsed), the recording is automatically stopped and finalized.
- **Motion Triggers Screen** (default: off): Also starts screen recording when motion is detected.

### Hotkey

- `Ctrl+Shift+ArrowUp` toggles Motion Mode on/off instantly. This updates settings and restarts/stops the detection loop.

### HUD Indicators

- **SCAN**: Amber pulsing dot — motion detection is active and scanning.
- **MOTION**: Amber flash — motion was just detected (flashes for 2 seconds).
- **REC**: Red dot — recording is active (takes priority over scan display).

### Motion Log

All motion detections are appended to `motion_log.txt` in the output directory with timestamps:

```text
2025-01-15_14-30-22 - Motion detected
2025-01-15_14-31-05 - Motion detected
```

### Analysis

The Analysis tab includes an "Analyze Last Motion Clip" button that runs the local analysis on the most recent `defEYE_webcam_*.mp4` capture.

### Privacy

Motion Mode is 100% local. The ffmpeg process runs entirely on-device. No frames, no detection data, and no network calls are made. All processing stays on the machine.

## Architecture Notes

- No tray plugin is used.
- The main settings window is created hidden, borderless, and skipped from the taskbar.
- Closing settings hides the window and keeps the process running.
- Autostart is enabled on first successful launch.
- Active webcam recording is held as one `Arc<parking_lot::Mutex<Option<std::process::Child>>>`.
- Active screen recording is held as a separate `Arc<parking_lot::Mutex<Option<std::process::Child>>>`.
- Multi-camera recordings are held in a `Vec<std::process::Child>` protected by `parking_lot::Mutex`.
- `recording_kind` is computed dynamically: `"screen"` takes priority over `"webcam"` when both are active.
- Screenshot merge uses virtual monitor coordinates, computes min/max bounds, creates one RGBA canvas, and copies every monitor image into the correct offset.
- Settings are serialized to `defeye_settings.json` in the app local data directory.
- Motion detection runs as a separate ffmpeg process with `select` + `showinfo` filter, parsed in a background thread.
- Motion state is tracked with `AtomicBool` for active flag and `Mutex<Option<Instant>>` for last detection time (cooldown).
- Motion detection is gracefully stopped via `q` on stdin + `kill()` on app exit or toggle off.
- Motion-triggered recordings reuse the same `recording_child` and `screen_recording_child` mutexes as manual recordings.
- Evidence hardening post-processing runs after recording stops: integrity sidecar, metadata sidecar, and thumbnail generation.
- SHA256 hashing uses the `sha2` crate for integrity verification.
- Camera cycling in quick-switch mode updates `active_camera_index` (AtomicUsize) and restarts recording if active.
- All child processes are gracefully stopped via a shared `graceful_stop_child` helper (stdin `q` + wait).
- Camera preview runs a separate ffmpeg process capturing JPEG frames to a temp file, polled by the frontend.
- Motion auto-stop monitor thread checks recording status every second and stops after post-record timeout.
- The StatusPill in the UI shows a live elapsed timer during recording using a React interval.
- The Captures tab header shows total file count and aggregate size.
- The About tab includes a keyboard shortcut reference card.
- Window dragging uses `getCurrentWindow().startDragging()` on mouse down via the cross icon in the header.
- Window minimize and hide buttons use `getCurrentWindow().minimize()` and `getCurrentWindow().hide()`.
- Stealth mode toggles window visibility via `window.hide()` / `window.show()` for all defEYE windows.
- Disk sentinel uses `GetDiskFreeSpaceExW` from the Windows API (`Win32_Storage_FileSystem` feature).
- Watchdog monitors recording child processes via `try_wait()` and auto-restarts with `start_recording_with_settings` / `start_screen_recording_with_settings`.
- Retention sentinel sorts captures by creation time and deletes oldest first to satisfy constraints.
- Time-lapse runs in a background thread with an `AtomicBool` active flag, sleeping in 1-second increments for responsive shutdown.
- Snapshot extraction uses ffmpeg with `-ss` seek and `-frames:v 1` for single-frame extraction.
- Capture annotations are stored as `*.note.json` sidecars with `{file, note, timestamp}` structure.
- `capture_kind_from_path` now delegates to `capture_kind` for consistent filename classification, returning `Option<&'static str>`.
- `capture_kind` recognizes `defEYE_timelapse_*` and `defEYE_snapshot_*` filenames as `"current"` kind.

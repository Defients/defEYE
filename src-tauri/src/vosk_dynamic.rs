//! Dynamic runtime loading of libvosk.dll — no link-time dependency.
//! If the DLL is not present, voice features are simply unavailable.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_short};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::OnceLock;

use libloading::Library;
use serde::Deserialize;

// Opaque C types
#[repr(C)]
pub struct VoskModel {
    _unused: [u8; 0],
}
#[repr(C)]
pub struct VoskRecognizer {
    _unused: [u8; 0],
}

// Function pointer types
type VoskModelNew = unsafe extern "C" fn(*const c_char) -> *mut VoskModel;
type VoskModelFree = unsafe extern "C" fn(*mut VoskModel);
type VoskRecognizerNew = unsafe extern "C" fn(*mut VoskModel, f32) -> *mut VoskRecognizer;
type VoskRecognizerNewGrm =
    unsafe extern "C" fn(*mut VoskModel, f32, *const c_char) -> *mut VoskRecognizer;
type VoskRecognizerAcceptWaveformS =
    unsafe extern "C" fn(*mut VoskRecognizer, *const c_short, c_int) -> c_int;
type VoskRecognizerPartialResult =
    unsafe extern "C" fn(*mut VoskRecognizer) -> *const c_char;
type VoskRecognizerResult =
    unsafe extern "C" fn(*mut VoskRecognizer) -> *const c_char;
type VoskRecognizerFree = unsafe extern "C" fn(*mut VoskRecognizer);
type VoskRecognizerSetMaxAlternatives = unsafe extern "C" fn(*mut VoskRecognizer, c_int);
type VoskRecognizerSetWords = unsafe extern "C" fn(*mut VoskRecognizer, c_int);
type VoskRecognizerSetPartialWords = unsafe extern "C" fn(*mut VoskRecognizer, c_int);

struct VoskLib {
    _lib: Library,
    model_new: VoskModelNew,
    model_free: VoskModelFree,
    recognizer_new: VoskRecognizerNew,
    recognizer_new_grm: VoskRecognizerNewGrm,
    accept_waveform_s: VoskRecognizerAcceptWaveformS,
    partial_result: VoskRecognizerPartialResult,
    result: VoskRecognizerResult,
    recognizer_free: VoskRecognizerFree,
    set_max_alternatives: VoskRecognizerSetMaxAlternatives,
    set_words: VoskRecognizerSetWords,
    set_partial_words: VoskRecognizerSetPartialWords,
}

static VOSK_LIB: OnceLock<Option<VoskLib>> = OnceLock::new();
static RESOURCE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Set the Tauri resource directory. Called from the Tauri setup hook
/// where `app.path().resource_dir()` is available.
pub fn set_resource_dir(dir: PathBuf) {
    eprintln!("[defEYE] Vosk resource dir set to: {}", dir.display());
    let _ = RESOURCE_DIR.set(Some(dir));
}

/// Set the Windows DLL search directory so libvosk.dll's dependencies
/// (libgcc_s_seh-1.dll, libstdc++-6.dll, libwinpthread-1.dll) can be found.
#[cfg(target_os = "windows")]
fn set_dll_directory(dir: &PathBuf) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        windows::Win32::System::LibraryLoader::SetDllDirectoryW(
            windows::core::PCWSTR(wide.as_ptr()),
        )
        .ok();
    }
}

#[cfg(not(target_os = "windows"))]
fn set_dll_directory(_dir: &PathBuf) {}

/// Collect candidate directories where libvosk.dll might be located.
fn dll_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // 1. Next to the executable (portable mode)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.to_path_buf());
        }
    }

    // 2. Tauri resource directory (from app.path().resource_dir())
    if let Some(res_dir) = RESOURCE_DIR.get().and_then(|opt| opt.as_ref()) {
        dirs.push(res_dir.clone());
    }

    // 3. Common Tauri resource subdirectories (installer mode)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs.push(exe_dir.join("resources"));
        }
    }

    // 4. Nested resource subdirectory (fallback for preserved path structure)
    if let Some(res_dir) = RESOURCE_DIR.get().and_then(|opt| opt.as_ref()) {
        dirs.push(
            res_dir
                .join("lib")
                .join("vosk-extracted")
                .join("vosk-win64-0.3.45"),
        );
    }

    dirs
}

/// Attempts to load libvosk.dll. Returns `false` if not found.
/// Must be called before any Model/Recognizer creation.
pub fn is_available() -> bool {
    VOSK_LIB
        .get_or_init(|| {
            let search_dirs = dll_search_dirs();

            eprintln!(
                "[defEYE] Searching for libvosk.dll in {} directories",
                search_dirs.len()
            );
            for dir in &search_dirs {
                let dll_path = dir.join("libvosk.dll");
                eprintln!("[defEYE]   checking: {}", dll_path.display());
                if dll_path.exists() {
                    eprintln!("[defEYE]   FOUND at {}", dll_path.display());
                    // Set the DLL search directory so dependency DLLs
                    // (libgcc_s_seh-1.dll, etc.) are found in the same folder
                    set_dll_directory(dir);

                    match unsafe { Library::new(&dll_path) } {
                        Ok(lib) => {
                            return load_symbols(lib);
                        }
                        Err(e) => {
                            eprintln!(
                                "[defEYE]   found but failed to load: {}",
                                e
                            );
                        }
                    }
                }
            }

            // Last resort: try system PATH
            eprintln!("[defEYE] trying system PATH for libvosk.dll");
            match unsafe { Library::new("libvosk.dll") } {
                Ok(lib) => load_symbols(lib),
                Err(e) => {
                    eprintln!("[defEYE] libvosk.dll not found anywhere: {}", e);
                    None
                }
            }
        })
        .is_some()
}

fn load_symbols(lib: Library) -> Option<VoskLib> {
    let load = |sym: &[u8]| -> Option<*mut ()> {
        let f = unsafe { lib.get::<unsafe extern "C" fn() -> *mut ()>(sym) }.ok()?;
        Some(unsafe { std::mem::transmute::<unsafe extern "C" fn() -> *mut (), *mut ()>(*f) })
    };

    let model_new = load(b"vosk_model_new")?;
    let model_free = load(b"vosk_model_free")?;
    let recognizer_new = load(b"vosk_recognizer_new")?;
    let recognizer_new_grm = load(b"vosk_recognizer_new_grm")?;
    let accept_waveform_s = load(b"vosk_recognizer_accept_waveform_s")?;
    let partial_result = load(b"vosk_recognizer_partial_result")?;
    let result = load(b"vosk_recognizer_result")?;
    let recognizer_free = load(b"vosk_recognizer_free")?;
    let set_max_alternatives = load(b"vosk_recognizer_set_max_alternatives")?;
    let set_words = load(b"vosk_recognizer_set_words")?;
    let set_partial_words = load(b"vosk_recognizer_set_partial_words")?;

    eprintln!("[defEYE] libvosk.dll loaded successfully");
    Some(VoskLib {
        _lib: lib,
        model_new: unsafe { std::mem::transmute(model_new) },
        model_free: unsafe { std::mem::transmute(model_free) },
        recognizer_new: unsafe { std::mem::transmute(recognizer_new) },
        recognizer_new_grm: unsafe { std::mem::transmute(recognizer_new_grm) },
        accept_waveform_s: unsafe { std::mem::transmute(accept_waveform_s) },
        partial_result: unsafe { std::mem::transmute(partial_result) },
        result: unsafe { std::mem::transmute(result) },
        recognizer_free: unsafe { std::mem::transmute(recognizer_free) },
        set_max_alternatives: unsafe { std::mem::transmute(set_max_alternatives) },
        set_words: unsafe { std::mem::transmute(set_words) },
        set_partial_words: unsafe { std::mem::transmute(set_partial_words) },
    })
}

fn vosk() -> &'static VoskLib {
    VOSK_LIB
        .get()
        .and_then(|opt| opt.as_ref())
        .expect("vosk library not loaded — call is_available() first")
}

// ---------------------------------------------------------------------------
// Public API — mirrors the `vosk` crate interface
// ---------------------------------------------------------------------------

pub struct Model(NonNull<VoskModel>);

impl Model {
    pub fn new(model_path: impl Into<String>) -> Option<Self> {
        if !is_available() {
            return None;
        }
        let lib = vosk();
        let path_c = CString::new(model_path.into()).ok()?;
        let ptr = unsafe { (lib.model_new)(path_c.as_ptr()) };
        NonNull::new(ptr).map(Self)
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        let lib = vosk();
        unsafe { (lib.model_free)(self.0.as_ptr()) }
    }
}

unsafe impl Send for Model {}
unsafe impl Sync for Model {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodingState {
    Finalized,
    Running,
    Failed,
}

impl DecodingState {
    fn from_c_int(value: c_int) -> Self {
        match value {
            1 => Self::Finalized,
            0 => Self::Running,
            _ => Self::Failed,
        }
    }
}

pub struct Recognizer(NonNull<VoskRecognizer>);

impl Recognizer {
    pub fn new(model: &Model, sample_rate: f32) -> Option<Self> {
        let lib = vosk();
        let ptr = unsafe { (lib.recognizer_new)(model.0.as_ptr(), sample_rate) };
        NonNull::new(ptr).map(Self)
    }

    pub fn new_with_grammar(
        model: &Model,
        sample_rate: f32,
        grammar: &[impl AsRef<str>],
    ) -> Option<Self> {
        let lib = vosk();
        let grammar_c = CString::new(format!(
            "[{}]",
            grammar
                .iter()
                .map(|phrase| format!("\"{}\"", phrase.as_ref()))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .ok()?;
        let ptr =
            unsafe { (lib.recognizer_new_grm)(model.0.as_ptr(), sample_rate, grammar_c.as_ptr()) };
        NonNull::new(ptr).map(Self)
    }

    pub fn accept_waveform(&mut self, data: &[i16]) -> DecodingState {
        let lib = vosk();
        let state =
            unsafe { (lib.accept_waveform_s)(self.0.as_ptr(), data.as_ptr(), data.len() as c_int) };
        DecodingState::from_c_int(state)
    }

    pub fn partial_result(&mut self) -> PartialResult<'_> {
        let lib = vosk();
        let ptr = unsafe { (lib.partial_result)(self.0.as_ptr()) };
        let json_str = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("");
        serde_json::from_str(json_str).unwrap_or(PartialResult {
            partial: "",
            partial_result: Vec::new(),
        })
    }

    pub fn set_max_alternatives(&mut self, max: u16) {
        let lib = vosk();
        unsafe { (lib.set_max_alternatives)(self.0.as_ptr(), max as c_int) }
    }

    pub fn set_words(&mut self, enable: bool) {
        let lib = vosk();
        unsafe { (lib.set_words)(self.0.as_ptr(), if enable { 1 } else { 0 }) }
    }

    pub fn set_partial_words(&mut self, enable: bool) {
        let lib = vosk();
        unsafe { (lib.set_partial_words)(self.0.as_ptr(), if enable { 1 } else { 0 }) }
    }

    pub fn result(&mut self) -> CompleteResult<'_> {
        let lib = vosk();
        let ptr = unsafe { (lib.result)(self.0.as_ptr()) };
        let json_str = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("");
        serde_json::from_str(json_str).unwrap_or(CompleteResult::Single(CompleteResultSingle {
            text: "",
            result: Vec::new(),
            speaker_info: None,
        }))
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        let lib = vosk();
        unsafe { (lib.recognizer_free)(self.0.as_ptr()) }
    }
}

unsafe impl Send for Recognizer {}
unsafe impl Sync for Recognizer {}

// ---------------------------------------------------------------------------
// Result types — mirrors the `vosk` crate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Word<'a> {
    pub conf: f32,
    pub start: f32,
    pub end: f32,
    pub word: &'a str,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct CompleteResultSingle<'a> {
    #[serde(flatten)]
    pub speaker_info: Option<SpeakerInfo>,
    #[serde(default)]
    pub result: Vec<Word<'a>>,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct SpeakerInfo {
    #[serde(rename = "spk")]
    pub vector: Vec<f32>,
    #[serde(rename = "spk_frames")]
    pub frames: u16,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompleteResult<'a> {
    #[serde(borrow)]
    Single(CompleteResultSingle<'a>),
    #[serde(borrow)]
    Multiple(CompleteResultMultiple<'a>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct CompleteResultMultiple<'a> {
    #[serde(borrow)]
    pub alternatives: Vec<Alternative<'a>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Alternative<'a> {
    pub confidence: f32,
    #[serde(default)]
    pub result: Vec<WordInAlternative<'a>>,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct WordInAlternative<'a> {
    pub start: f32,
    pub end: f32,
    pub word: &'a str,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct PartialResult<'a> {
    #[serde(borrow, default)]
    pub partial: &'a str,
    #[serde(borrow, default)]
    pub partial_result: Vec<Word<'a>>,
}

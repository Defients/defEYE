fn main() {
    // Vosk is loaded dynamically at runtime via libloading — no link-time
    // dependency on libvosk.lib. Voice features are optional.

    tauri_build::build();
}

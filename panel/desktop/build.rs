//! `tauri_build`: konfiguracja (`tauri.conf.json`), ikony, okno połączenia (`ui/`) i uprawnienie komendy `ipc`.
fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["ipc"])),
    )
    .expect("tauri_build");
}

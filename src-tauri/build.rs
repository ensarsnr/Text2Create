fn main() {
    // Every application command must be listed here to get `allow-<command>` and
    // `deny-<command>` permissions. Because an application manifest exists, Tauri
    // rejects any application command that no capability explicitly allows.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "greet",
            "create_project",
            "list_projects",
            "open_project",
            "apply_project_changes",
            "close_project",
        ]),
    ))
    .expect("failed to run tauri-build");
}

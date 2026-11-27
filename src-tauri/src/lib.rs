//! Text2Create application core.

mod projects;

use std::sync::Mutex;
use tauri::Manager;

/// Maximum accepted length, in characters, for the demo `greet` input.
const MAX_NAME_LENGTH: usize = 100;

/// Demo command that verifies frontend-to-Rust communication.
///
/// Input from the frontend is treated as untrusted and validated.
#[tauri::command]
fn greet(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name must not be empty.".into());
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Err(format!(
            "Name must be at most {MAX_NAME_LENGTH} characters."
        ));
    }
    Ok(format!(
        "Hello, {name}! This reply was produced by the Rust core."
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // ~/Library/Application Support/<bundle identifier>/projects on macOS.
            let root = app
                .path()
                .app_data_dir()?
                .join(projects::PROJECTS_DIRECTORY);
            app.manage::<projects::ProjectState>(Mutex::new(projects::ProjectService::new(root)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            projects::create_project,
            projects::list_projects,
            projects::open_project,
            projects::apply_project_changes,
            projects::close_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Text2Create");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_returns_reply_for_valid_name() {
        assert_eq!(
            greet("  Ada ").unwrap(),
            "Hello, Ada! This reply was produced by the Rust core."
        );
    }

    #[test]
    fn greet_rejects_empty_name() {
        assert!(greet("   ").is_err());
    }

    #[test]
    fn greet_rejects_too_long_name() {
        assert!(greet(&"a".repeat(MAX_NAME_LENGTH + 1)).is_err());
    }
}

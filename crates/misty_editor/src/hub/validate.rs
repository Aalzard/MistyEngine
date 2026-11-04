//! Project name and location checks for the New Project page.

use std::path::{Path, PathBuf};

/// Names Windows reserves for devices (also with an extension).
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
pub const MAX_NAME: usize = 64;

/// Why `name` can't be a project folder name, if it can't.
pub fn name_error(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some("Enter a project name.".into());
    }
    if trimmed.chars().count() > MAX_NAME {
        return Some(format!("Use at most {MAX_NAME} characters."));
    }
    if let Some(c) = trimmed
        .chars()
        .find(|c| "<>:\"/\\|?*".contains(*c) || c.is_control())
    {
        return Some(format!("A project name can't contain '{c}'."));
    }
    if trimmed.ends_with('.') {
        return Some("A project name can't end with a dot.".into());
    }
    let stem = trimmed.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Some(format!("\"{trimmed}\" is reserved by the system."));
    }
    None
}

/// The folder the project would be created in, or why it can't be.
pub fn project_folder(name: &str, location: &Path) -> Result<PathBuf, String> {
    if let Some(e) = name_error(name) {
        return Err(e);
    }
    if location.as_os_str().is_empty() {
        return Err("Choose a location.".into());
    }
    if location.exists() && !location.is_dir() {
        return Err("The location is a file, not a folder.".into());
    }
    let folder = location.join(name.trim());
    if folder.exists() {
        return Err(format!(
            "A folder named \"{}\" already exists there.",
            name.trim()
        ));
    }
    Ok(folder)
}

/// "New Project", or "New Project 2", 3, … : the first name not taken in `location`.
pub fn unique_name(base: &str, location: &Path) -> String {
    if !location.join(base).exists() {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !location.join(n).exists())
        .unwrap_or_else(|| base.to_owned())
}

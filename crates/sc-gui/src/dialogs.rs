//! Native file dialogs. Only wired up on Windows; elsewhere folders are set in
//! the config file.

use std::path::{Path, PathBuf};

/// Ask the user to choose a folder, starting at `start`. `None` if cancelled
/// or unavailable.
#[cfg(windows)]
pub fn pick_folder(start: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Choose a folder");
    if start.is_dir() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder()
}

#[cfg(not(windows))]
pub fn pick_folder(_start: &Path) -> Option<PathBuf> {
    None
}

/// Whether `pick_folder` can do anything on this platform.
pub const fn available() -> bool {
    cfg!(windows)
}

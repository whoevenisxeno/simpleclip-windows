use std::path::{Path, PathBuf};

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

pub const fn available() -> bool {
    cfg!(windows)
}

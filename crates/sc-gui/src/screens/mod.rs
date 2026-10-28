pub mod dashboard;
#[cfg(windows)]
pub mod editor;
#[cfg(not(windows))]
#[path = "editor_stub.rs"]
pub mod editor;
pub mod gallery;
pub mod settings;
pub mod share;
pub mod sheet;
pub mod wizard;

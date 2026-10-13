use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("clipboard error: {0}")]
    Clipboard(String),
    #[error("key simulation failed: {0}")]
    KeySim(String),
    #[error("unsupported platform: {0}")]
    Unsupported(String),
    #[error("app query failed: {0}")]
    AppQuery(String),
}

#[derive(Debug, Clone, Default)]
pub struct SavedClip {
    pub text: Option<String>,
}

pub trait Platform: Send + Sync {
    fn current_app_pid(&self) -> Result<u32, PlatformError>;
    fn save_clipboard(&self) -> Result<SavedClip, PlatformError>;
    fn restore_clipboard(&self, clip: SavedClip) -> Result<(), PlatformError>;
    fn copy_selection(&self) -> Result<Option<String>, PlatformError>;
    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError>;
    fn paste_text(&self, text: &str) -> Result<(), PlatformError>;
}

#[cfg(not(target_os = "macos"))]
pub struct UnsupportedPlatform;

#[cfg(not(target_os = "macos"))]
fn macos_only<T>() -> Result<T, PlatformError> {
    Err(PlatformError::Unsupported(
        "proofd MVP is macOS-only".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
impl Platform for UnsupportedPlatform {
    fn current_app_pid(&self) -> Result<u32, PlatformError> {
        macos_only()
    }
    fn save_clipboard(&self) -> Result<SavedClip, PlatformError> {
        macos_only()
    }
    fn restore_clipboard(&self, _clip: SavedClip) -> Result<(), PlatformError> {
        macos_only()
    }
    fn copy_selection(&self) -> Result<Option<String>, PlatformError> {
        macos_only()
    }
    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError> {
        macos_only()
    }
    fn paste_text(&self, _text: &str) -> Result<(), PlatformError> {
        macos_only()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn build_platform() -> Result<UnsupportedPlatform, PlatformError> {
    Err(PlatformError::Unsupported(
        "proofd MVP is macOS-only; build on macOS".to_string(),
    ))
}

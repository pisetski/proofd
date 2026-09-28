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

/// Saved plaintext clipboard contents (MVP: plaintext only).
#[derive(Debug, Clone, Default)]
pub struct SavedClip {
    pub text: Option<String>,
}

/// Platform abstraction. Never mutates app text with AXSetValue;
/// replacement is via simulated Cmd+V only.
pub trait Platform: Send + Sync {
    fn current_app_pid(&self) -> Result<u32, PlatformError>;
    fn save_clipboard(&self) -> Result<SavedClip, PlatformError>;
    fn restore_clipboard(&self, clip: SavedClip) -> Result<(), PlatformError>;
    fn copy_selection(&self) -> Result<Option<String>, PlatformError>;
    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError>;
    fn paste_text(&self, text: &str) -> Result<(), PlatformError>;
    fn set_clipboard_text(&self, text: &str) -> Result<(), PlatformError>;
}

/// Non-macOS stub: returns a clear unsupported-platform error.
#[cfg(not(target_os = "macos"))]
pub struct UnsupportedPlatform;

#[cfg(not(target_os = "macos"))]
impl Platform for UnsupportedPlatform {
    fn current_app_pid(&self) -> Result<u32, PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn save_clipboard(&self) -> Result<SavedClip, PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn restore_clipboard(&self, _clip: SavedClip) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn copy_selection(&self) -> Result<Option<String>, PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn paste_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
    fn set_clipboard_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "proofd MVP is macOS-only".to_string(),
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn build_platform() -> Result<UnsupportedPlatform, PlatformError> {
    Err(PlatformError::Unsupported(
        "proofd MVP is macOS-only; build on macOS".to_string(),
    ))
}

//! macOS platform primitives.
//!
//! - Key simulation via `enigo` (Cmd+C / Cmd+A / Cmd+V).
//! - Plaintext clipboard via `arboard` (MVP: rich contents NOT preserved).
//! - Front app PID via `lsappinfo` (fast, ~10ms), with `osascript`
//!   (System Events) as fallback.
//! - Never mutates app text with AXSetValue; replacement is Cmd+V only.
//! - Fallback `copy_all_focused_text` uses Cmd+A, Cmd+C only after selection
//!   copy returns empty. It depends on focused text-field behavior and may
//!   select more than intended if focus is not in a text field; daemon caps
//!   input with `max_input_chars`.

use crate::platform::{Platform, PlatformError, SavedClip};
use std::sync::Mutex;
use std::time::Duration;

const AFTER_COPY_DELAY: Duration = Duration::from_millis(100);
const AFTER_PASTE_DELAY: Duration = Duration::from_millis(200);
const BETWEEN_KEYS_DELAY: Duration = Duration::from_millis(25);
const CLIPBOARD_SETTLE_DELAY: Duration = Duration::from_millis(15);
const PASTE_SETTLE_DELAY: Duration = Duration::from_millis(30);

pub struct MacosPlatform {
    /// Lazily initialized on first key simulation, and retried on every use
    /// while missing: granting Accessibility later heals the daemon without
    /// a restart, and a missing grant fails per-press (notify + log) instead
    /// of crash-looping the whole daemon at startup.
    enigo: Mutex<Option<enigo::Enigo>>,
}

impl Default for MacosPlatform {
    fn default() -> Self {
        Self {
            enigo: Mutex::new(None),
        }
    }
}

impl MacosPlatform {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_enigo<T>(
        &self,
        f: impl FnOnce(&mut enigo::Enigo) -> Result<T, PlatformError>,
    ) -> Result<T, PlatformError> {
        let mut guard = self
            .enigo
            .lock()
            .map_err(|e| PlatformError::KeySim(format!("enigo lock: {e}")))?;
        if guard.is_none() {
            let created = enigo::Enigo::new(&enigo::Settings::default())
                .map_err(|e| PlatformError::KeySim(format!("enigo init: {e}")))?;
            *guard = Some(created);
        }
        f(guard.as_mut().expect("enigo just initialized"))
    }

    fn cmd_click(&self, ch: char) -> Result<(), PlatformError> {
        use enigo::{Direction, Key, Keyboard};
        self.with_enigo(|enigo| {
            enigo
                .key(Key::Meta, Direction::Press)
                .map_err(|e| PlatformError::KeySim(format!("cmd press: {e}")))?;
            std::thread::sleep(BETWEEN_KEYS_DELAY);
            enigo
                .key(Key::Unicode(ch), Direction::Click)
                .map_err(|e| PlatformError::KeySim(format!("cmd+{ch}: {e}")))?;
            std::thread::sleep(BETWEEN_KEYS_DELAY);
            enigo
                .key(Key::Meta, Direction::Release)
                .map_err(|e| PlatformError::KeySim(format!("cmd release: {e}")))?;
            Ok(())
        })
    }

    fn clipboard_get_text() -> Option<String> {
        let mut cb = arboard::Clipboard::new().ok()?;
        match cb.get_text() {
            Ok(t) if !t.is_empty() => Some(t),
            _ => None,
        }
    }

    fn clipboard_set_text(text: &str) -> Result<(), PlatformError> {
        let mut cb =
            arboard::Clipboard::new().map_err(|e| PlatformError::Clipboard(e.to_string()))?;
        cb.set_text(text)
            .map_err(|e| PlatformError::Clipboard(e.to_string()))
    }

    fn clipboard_clear() {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.clear();
        }
    }
}

impl Platform for MacosPlatform {
    fn current_app_pid(&self) -> Result<u32, PlatformError> {
        // Fast path: `lsappinfo` (~10ms for both spawns) vs `osascript`
        // (~130ms + AppleScript interpreter). Fall back to osascript if the
        // output format ever changes.
        if let Ok(pid) = front_pid_via_lsappinfo() {
            return Ok(pid);
        }
        front_pid_via_osascript()
    }

    fn save_clipboard(&self) -> Result<SavedClip, PlatformError> {
        Ok(SavedClip {
            text: Self::clipboard_get_text(),
        })
    }

    fn restore_clipboard(&self, clip: SavedClip) -> Result<(), PlatformError> {
        match clip.text {
            Some(t) => Self::clipboard_set_text(&t),
            None => {
                Self::clipboard_clear();
                Ok(())
            }
        }
    }

    fn set_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        Self::clipboard_set_text(text)
    }

    fn copy_selection(&self) -> Result<Option<String>, PlatformError> {
        // Clear first so an empty selection is distinguishable from
        // "clipboard unchanged".
        Self::clipboard_clear();
        std::thread::sleep(CLIPBOARD_SETTLE_DELAY);
        self.cmd_click('c')?;
        std::thread::sleep(AFTER_COPY_DELAY);
        Ok(Self::clipboard_get_text())
    }

    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError> {
        // Only call after selection copy returned empty (daemon enforces).
        self.cmd_click('a')?;
        std::thread::sleep(BETWEEN_KEYS_DELAY);
        Self::clipboard_clear();
        std::thread::sleep(CLIPBOARD_SETTLE_DELAY);
        self.cmd_click('c')?;
        std::thread::sleep(AFTER_COPY_DELAY);
        Ok(Self::clipboard_get_text())
    }

    fn paste_text(&self, text: &str) -> Result<(), PlatformError> {
        Self::clipboard_set_text(text)?;
        // Small settle before pasting; restoring too soon pastes old contents.
        std::thread::sleep(PASTE_SETTLE_DELAY);
        self.cmd_click('v')?;
        std::thread::sleep(AFTER_PASTE_DELAY);
        Ok(())
    }
}

/// Fast front-app PID via two tiny `lsappinfo` spawns (~10ms total).
fn front_pid_via_lsappinfo() -> Result<u32, PlatformError> {
    let asn_out = std::process::Command::new("/usr/bin/lsappinfo")
        .arg("front")
        .output()
        .map_err(|e| PlatformError::AppQuery(format!("lsappinfo front spawn: {e}")))?;
    if !asn_out.status.success() {
        return Err(PlatformError::AppQuery(format!(
            "lsappinfo front failed: {}",
            String::from_utf8_lossy(&asn_out.stderr).trim()
        )));
    }
    let asn = String::from_utf8_lossy(&asn_out.stdout);
    let asn = asn.trim();
    if asn.is_empty() {
        return Err(PlatformError::AppQuery("lsappinfo front: empty ASN".into()));
    }
    let pid_out = std::process::Command::new("/usr/bin/lsappinfo")
        .args(["info", "-only", "pid", "-asn", asn])
        .output()
        .map_err(|e| PlatformError::AppQuery(format!("lsappinfo info spawn: {e}")))?;
    if !pid_out.status.success() {
        return Err(PlatformError::AppQuery(format!(
            "lsappinfo info failed: {}",
            String::from_utf8_lossy(&pid_out.stderr).trim()
        )));
    }
    parse_lsappinfo_pid(&String::from_utf8_lossy(&pid_out.stdout))
}

/// Legacy front-app PID via AppleScript (~130ms); fallback only.
fn front_pid_via_osascript() -> Result<u32, PlatformError> {
    let out = std::process::Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to get unix id of (first process where frontmost is true)",
        ])
        .output()
        .map_err(|e| PlatformError::AppQuery(format!("osascript spawn: {e}")))?;
    if !out.status.success() {
        return Err(PlatformError::AppQuery(format!(
            "osascript failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim()
        .parse::<u32>()
        .map_err(|e| PlatformError::AppQuery(format!("parse front pid {:?}: {e}", s.trim())))
}

/// Parse `"pid"=1234` output of `lsappinfo info -only pid`.
fn parse_lsappinfo_pid(s: &str) -> Result<u32, PlatformError> {
    let t = s.trim().trim_matches('"');
    let num = t.rsplit('=').next().unwrap_or("").trim().trim_matches('"');
    num.parse::<u32>()
        .map_err(|e| PlatformError::AppQuery(format!("parse lsappinfo pid {s:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsappinfo_pid_output() {
        assert_eq!(parse_lsappinfo_pid("\"pid\"=1110\n").unwrap(), 1110);
        assert_eq!(parse_lsappinfo_pid("  \"pid\"=41086  ").unwrap(), 41086);
        assert!(parse_lsappinfo_pid("").is_err());
        assert!(parse_lsappinfo_pid("ASN:0x0-0x43043:").is_err());
    }

    #[test]
    fn front_pid_matches_osascript() {
        // Both backends must agree on the front app. Skipped when neither
        // helper is available (e.g. headless CI).
        let fast = front_pid_via_lsappinfo();
        let slow = front_pid_via_osascript();
        match (fast, slow) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            _ => eprintln!("skipping: lsappinfo or osascript unavailable"),
        }
    }
}

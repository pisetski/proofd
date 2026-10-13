use crate::platform::{Platform, PlatformError, SavedClip};
use std::sync::Mutex;
use std::time::Duration;

const AFTER_COPY_DELAY: Duration = Duration::from_millis(100);
const AFTER_PASTE_DELAY: Duration = Duration::from_millis(200);
const BETWEEN_KEYS_DELAY: Duration = Duration::from_millis(25);
const CLIPBOARD_SETTLE_DELAY: Duration = Duration::from_millis(15);
const PASTE_SETTLE_DELAY: Duration = Duration::from_millis(30);

pub struct MacosPlatform {
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

    fn copy_selection(&self) -> Result<Option<String>, PlatformError> {
        Self::clipboard_clear();
        std::thread::sleep(CLIPBOARD_SETTLE_DELAY);
        self.cmd_click('c')?;
        std::thread::sleep(AFTER_COPY_DELAY);
        Ok(Self::clipboard_get_text())
    }

    fn copy_all_focused_text(&self) -> Result<Option<String>, PlatformError> {
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
        std::thread::sleep(PASTE_SETTLE_DELAY);
        self.cmd_click('v')?;
        std::thread::sleep(AFTER_PASTE_DELAY);
        Ok(())
    }
}

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
}

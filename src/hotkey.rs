use core_graphics::event::{CGEventFlags, CGKeyCode, KeyCode};
use std::fmt;

const SHIFT_BIT: u64 = 0x00020000;
const CTRL_BIT: u64 = 0x00040000;
const ALT_BIT: u64 = 0x00080000;
const CMD_BIT: u64 = 0x00100000;
const MOD_MASK_BITS: u64 = SHIFT_BIT | CTRL_BIT | ALT_BIT | CMD_BIT;
const MODIFIER_LABELS_IN_ORDER: [(u64, &str); 4] = [
    (CTRL_BIT, "ctrl"),
    (SHIFT_BIT, "shift"),
    (ALT_BIT, "alt"),
    (CMD_BIT, "cmd"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyCombo {
    mods: CGEventFlags,
    keycode: CGKeyCode,
    label: String,
}

impl HotkeyCombo {
    pub fn matches(&self, flags: CGEventFlags, keycode: CGKeyCode) -> bool {
        (flags.bits() & MOD_MASK_BITS) == self.mods.bits() && keycode == self.keycode
    }
}

impl fmt::Display for HotkeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (keycode={:#x} mods={:#x})",
            self.label,
            self.keycode,
            self.mods.bits()
        )
    }
}

pub fn default_combo() -> HotkeyCombo {
    parse_combo("Ctrl-Alt-P").expect("default combo must parse")
}

pub fn parse_combo(s: &str) -> Result<HotkeyCombo, String> {
    let mut mods_bits: u64 = 0;
    let mut key: Option<(CGKeyCode, String)> = None;
    for raw in s.split(['+', '-']) {
        let t = raw.trim().to_uppercase();
        if t.is_empty() {
            return Err(format!("invalid hotkey {s:?}: empty part"));
        }
        match t.as_str() {
            "CTRL" | "CONTROL" => mods_bits |= CTRL_BIT,
            "ALT" | "OPTION" => mods_bits |= ALT_BIT,
            "SHIFT" => mods_bits |= SHIFT_BIT,
            "CMD" | "COMMAND" | "SUPER" => mods_bits |= CMD_BIT,
            _ => {
                if key.is_some() {
                    return Err(format!("invalid hotkey {s:?}: multiple keys"));
                }
                let code = parse_key(&t)
                    .ok_or_else(|| format!("invalid hotkey {s:?}: unknown key {raw:?}"))?;
                let short = t
                    .strip_prefix("KEY")
                    .or_else(|| t.strip_prefix("ANSI_"))
                    .unwrap_or(&t)
                    .to_lowercase();
                key = Some((code, short));
            }
        }
    }
    let Some((keycode, key_name)) = key else {
        return Err(format!("invalid hotkey {s:?}: no key"));
    };
    let label = MODIFIER_LABELS_IN_ORDER
        .iter()
        .filter(|(bit, _)| mods_bits & bit != 0)
        .map(|(_, name)| name.to_string())
        .chain(std::iter::once(key_name))
        .collect::<Vec<_>>()
        .join("+");
    Ok(HotkeyCombo {
        mods: CGEventFlags::from_bits_truncate(mods_bits),
        keycode,
        label,
    })
}

fn parse_key(t: &str) -> Option<CGKeyCode> {
    let name = t
        .strip_prefix("KEY")
        .or_else(|| t.strip_prefix("ANSI_"))
        .unwrap_or(t);
    match name {
        "A" => Some(KeyCode::ANSI_A),
        "B" => Some(KeyCode::ANSI_B),
        "C" => Some(KeyCode::ANSI_C),
        "D" => Some(KeyCode::ANSI_D),
        "E" => Some(KeyCode::ANSI_E),
        "F" => Some(KeyCode::ANSI_F),
        "G" => Some(KeyCode::ANSI_G),
        "H" => Some(KeyCode::ANSI_H),
        "I" => Some(KeyCode::ANSI_I),
        "J" => Some(KeyCode::ANSI_J),
        "K" => Some(KeyCode::ANSI_K),
        "L" => Some(KeyCode::ANSI_L),
        "M" => Some(KeyCode::ANSI_M),
        "N" => Some(KeyCode::ANSI_N),
        "O" => Some(KeyCode::ANSI_O),
        "P" => Some(KeyCode::ANSI_P),
        "Q" => Some(KeyCode::ANSI_Q),
        "R" => Some(KeyCode::ANSI_R),
        "S" => Some(KeyCode::ANSI_S),
        "T" => Some(KeyCode::ANSI_T),
        "U" => Some(KeyCode::ANSI_U),
        "V" => Some(KeyCode::ANSI_V),
        "W" => Some(KeyCode::ANSI_W),
        "X" => Some(KeyCode::ANSI_X),
        "Y" => Some(KeyCode::ANSI_Y),
        "Z" => Some(KeyCode::ANSI_Z),
        "0" => Some(KeyCode::ANSI_0),
        "1" => Some(KeyCode::ANSI_1),
        "2" => Some(KeyCode::ANSI_2),
        "3" => Some(KeyCode::ANSI_3),
        "4" => Some(KeyCode::ANSI_4),
        "5" => Some(KeyCode::ANSI_5),
        "6" => Some(KeyCode::ANSI_6),
        "7" => Some(KeyCode::ANSI_7),
        "8" => Some(KeyCode::ANSI_8),
        "9" => Some(KeyCode::ANSI_9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> CGEventFlags {
        CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagShift
    }

    #[test]
    fn parses_common_styles() {
        let c = parse_combo("Ctrl-Shift-Y").unwrap();
        assert_eq!(c.keycode, KeyCode::ANSI_Y);
        assert!(c.matches(flags(), KeyCode::ANSI_Y));
        assert_eq!(parse_combo("ctrl+shift+y").unwrap(), c);
        assert_eq!(parse_combo("Control-Shift-KeyY").unwrap(), c);
    }

    #[test]
    fn parses_option_alias_and_default() {
        let a = parse_combo("Ctrl-Alt-P").unwrap();
        let b = parse_combo("Ctrl-Option-P").unwrap();
        assert_eq!(a, b);
        assert_eq!(default_combo(), a);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_combo("").is_err());
        assert!(parse_combo("Ctrl-Shift").is_err());
        assert!(parse_combo("Ctrl-Y-Z").is_err());
        assert!(parse_combo("Ctrl-Frob").is_err());
        assert!(parse_combo("Ctrl--Y").is_err());
    }

    #[test]
    fn ignores_incidental_flag_bits() {
        let c = parse_combo("Ctrl-Shift-Y").unwrap();
        let noisy = flags()
            | CGEventFlags::CGEventFlagAlphaShift
            | CGEventFlags::CGEventFlagSecondaryFn
            | CGEventFlags::CGEventFlagNonCoalesced
            | CGEventFlags::CGEventFlagNumericPad;
        assert!(c.matches(noisy, KeyCode::ANSI_Y));
    }

    #[test]
    fn rejects_wrong_mods_or_key() {
        let c = parse_combo("Ctrl-Shift-Y").unwrap();
        assert!(!c.matches(CGEventFlags::CGEventFlagControl, KeyCode::ANSI_Y));
        assert!(!c.matches(
            CGEventFlags::CGEventFlagControl
                | CGEventFlags::CGEventFlagShift
                | CGEventFlags::CGEventFlagAlternate,
            KeyCode::ANSI_Y
        ));
        assert!(!c.matches(flags(), KeyCode::ANSI_U));
    }
}

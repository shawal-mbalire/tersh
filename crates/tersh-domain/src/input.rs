//! Terminal input encoding: turning key events, mouse reports, wheel movement
//! and pasted text into VT byte sequences according to the active [`mode`] bits
//! (matching `alacritty` and the Kitty keyboard protocol).

use serde::Deserialize;

/// `alacritty_terminal::term::TermMode` bit flags.
pub mod mode {
    pub const SHOW_CURSOR: u32 = 1 << 0;
    pub const APP_CURSOR: u32 = 1 << 1;
    pub const APP_KEYPAD: u32 = 1 << 2;
    pub const MOUSE_REPORT_CLICK: u32 = 1 << 3;
    pub const BRACKETED_PASTE: u32 = 1 << 4;
    pub const SGR_MOUSE: u32 = 1 << 5;
    pub const MOUSE_MOTION: u32 = 1 << 6;
    pub const LINE_WRAP: u32 = 1 << 7;
    pub const LINE_FEED_NEW_LINE: u32 = 1 << 8;
    pub const ORIGIN: u32 = 1 << 9;
    pub const INSERT: u32 = 1 << 10;
    pub const FOCUS_IN_OUT: u32 = 1 << 11;
    pub const ALT_SCREEN: u32 = 1 << 12;
    pub const MOUSE_DRAG: u32 = 1 << 13;
    pub const UTF8_MOUSE: u32 = 1 << 14;
    pub const ALTERNATE_SCROLL: u32 = 1 << 15;
    pub const DISAMBIGUATE_KEYS: u32 = 1 << 18;
    pub const REPORT_EVENT_TYPES: u32 = 1 << 19;
    pub const REPORT_ALTERNATE_KEYS: u32 = 1 << 20;
    pub const REPORT_ALL_KEYS_AS_ESC: u32 = 1 << 21;
    pub const REPORT_ASSOCIATED_TEXT: u32 = 1 << 22;

    pub const MOUSE_MODE: u32 = MOUSE_REPORT_CLICK | MOUSE_MOTION | MOUSE_DRAG;
    pub const KITTY_KEYBOARD_PROTOCOL: u32 = DISAMBIGUATE_KEYS
        | REPORT_EVENT_TYPES
        | REPORT_ALTERNATE_KEYS
        | REPORT_ALL_KEYS_AS_ESC
        | REPORT_ASSOCIATED_TEXT;
}

const ESC: &str = "\x1b";
const CSI: &str = "\x1b[";
const SS3: &str = "\x1bO";

/// A keyboard event received from the window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct KeyInput {
    pub key: String,
    pub code: String,
    pub location: u8,
    pub repeat: bool,
    /// `"keydown"` (default) or `"keyup"`.
    #[serde(rename = "type")]
    pub event_type: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// xterm modifier parameter: `1 + shift + 2*alt + 4*ctrl + 8*meta`.
fn modifier(e: &KeyInput) -> u8 {
    1 + u8::from(e.shift) + 2 * u8::from(e.alt) + 4 * u8::from(e.ctrl) + 8 * u8::from(e.meta)
}

fn cursor_key(key: &str) -> Option<char> {
    match key {
        "ArrowUp" => Some('A'),
        "ArrowDown" => Some('B'),
        "ArrowRight" => Some('C'),
        "ArrowLeft" => Some('D'),
        "Home" => Some('H'),
        "End" => Some('F'),
        _ => None,
    }
}

fn tilde_key(key: &str) -> Option<u8> {
    match key {
        "Insert" => Some(2),
        "Delete" => Some(3),
        "PageUp" => Some(5),
        "PageDown" => Some(6),
        "F5" => Some(15),
        "F6" => Some(17),
        "F7" => Some(18),
        "F8" => Some(19),
        "F9" => Some(20),
        "F10" => Some(21),
        "F11" => Some(23),
        "F12" => Some(24),
        _ => None,
    }
}

fn ss3_fkey(key: &str) -> Option<char> {
    match key {
        "F1" => Some('P'),
        "F2" => Some('Q'),
        "F3" => Some('R'),
        "F4" => Some('S'),
        _ => None,
    }
}

fn kitty_functional_key(key: &str) -> Option<u32> {
    match key {
        "Escape" => Some(27),
        "Enter" => Some(13),
        "Tab" => Some(9),
        "Backspace" => Some(127),
        "CapsLock" => Some(57358),
        "ScrollLock" => Some(57359),
        "NumLock" => Some(57360),
        "PrintScreen" => Some(57361),
        "Pause" => Some(57362),
        "ContextMenu" => Some(57363),
        "F13" => Some(57376),
        "F14" => Some(57377),
        "F15" => Some(57378),
        "F16" => Some(57379),
        "F17" => Some(57380),
        "F18" => Some(57381),
        "F19" => Some(57382),
        "F20" => Some(57383),
        "F21" => Some(57384),
        "F22" => Some(57385),
        "F23" => Some(57386),
        "F24" => Some(57387),
        _ => None,
    }
}

fn modifier_key_code(key: &str, code: &str, location: u8) -> Option<u32> {
    match code {
        "ShiftLeft" => return Some(57441),
        "ControlLeft" => return Some(57442),
        "AltLeft" => return Some(57443),
        "MetaLeft" => return Some(57444),
        "ShiftRight" => return Some(57447),
        "ControlRight" => return Some(57448),
        "AltRight" => return Some(57449),
        "MetaRight" => return Some(57450),
        "AltGraph" => return Some(57453),
        _ => {},
    }
    let right = location == 2;
    match key {
        "Shift" => Some(if right { 57447 } else { 57441 }),
        "Control" => Some(if right { 57448 } else { 57442 }),
        "Alt" => Some(if right { 57449 } else { 57443 }),
        "Meta" | "Super" => Some(if right { 57450 } else { 57444 }),
        "AltGraph" => Some(57453),
        _ => None,
    }
}

fn code_base_char(code: &str) -> Option<char> {
    match code {
        "Backquote" => Some('`'),
        "Minus" => Some('-'),
        "Equal" => Some('='),
        "BracketLeft" => Some('['),
        "BracketRight" => Some(']'),
        "Backslash" => Some('\\'),
        "Semicolon" => Some(';'),
        "Quote" => Some('\''),
        "Comma" => Some(','),
        "Period" => Some('.'),
        "Slash" => Some('/'),
        _ => None,
    }
}

/// Resolve the logical key name when the platform reports `"Unidentified"`
/// (e.g. `GDK_KEY_ISO_Left_Tab` on Linux for `Shift+Tab`, where `code` is `"Tab"`).
pub fn effective_key(e: &KeyInput) -> String {
    if !e.key.is_empty() && e.key != "Unidentified" {
        return e.key.clone();
    }
    let code = e.code.as_str();
    if code.is_empty() {
        return e.key.clone();
    }
    match code {
        "Tab" => return "Tab".into(),
        "Enter" | "NumpadEnter" => return "Enter".into(),
        "Backspace" => return "Backspace".into(),
        "Escape" => return "Escape".into(),
        "Space" => return " ".into(),
        _ => {},
    }
    if let Some(rest) = code.strip_prefix("Key") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase() {
            return if e.shift { rest.into() } else { rest.to_ascii_lowercase() };
        }
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
            return rest.into();
        }
    }
    if let Some(ch) = code_base_char(code) {
        return ch.to_string();
    }
    if cursor_key(code).is_some()
        || tilde_key(code).is_some()
        || ss3_fkey(code).is_some()
        || kitty_functional_key(code).is_some()
    {
        return code.into();
    }
    e.key.clone()
}

fn base_and_shifted_code(key: &str, code: &str, shift: bool) -> (u32, Option<u32>) {
    let cp = key.chars().next().unwrap_or('\0') as u32;
    let mut base_char = key.chars().next().unwrap_or('\0').to_lowercase().next().unwrap_or('\0');
    if let Some(rest) = code.strip_prefix("Key") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase() {
            base_char = (rest.as_bytes()[0] as char).to_ascii_lowercase();
        }
    } else if let Some(rest) = code.strip_prefix("Digit") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
            base_char = rest.as_bytes()[0] as char;
        }
    } else if let Some(ch) = code_base_char(code) {
        base_char = ch;
    }
    let base = base_char as u32;
    let shifted = (shift && cp != base).then_some(cp);
    (base, shifted)
}

fn format_kitty_u(key_param: &str, mods: u8, event_suffix: &str, associated_text: Option<u32>) -> String {
    let mod_part = if mods > 1 || !event_suffix.is_empty() {
        format!("{mods}{event_suffix}")
    } else {
        String::new()
    };
    if let Some(text) = associated_text {
        format!("{CSI}{key_param};{mod_part};{text}u")
    } else if !mod_part.is_empty() {
        format!("{CSI}{key_param};{mod_part}u")
    } else {
        format!("{CSI}{key_param}u")
    }
}

/// Ctrl + key -> C0 control character.
fn control_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let c = ch.to_ascii_lowercase();
    if c.is_ascii_lowercase() {
        return Some((c as u8 - 96) as char);
    }
    match c {
        '@' | ' ' | '2' => Some('\x00'),
        '[' | '3' => Some('\x1b'),
        '\\' | '4' => Some('\x1c'),
        ']' | '5' => Some('\x1d'),
        '^' | '6' => Some('\x1e'),
        '_' | '-' | '7' | '/' => Some('\x1f'),
        '?' | '8' => Some('\x7f'),
        _ => None,
    }
}

/// Encode a key press (or release) as VT bytes for the PTY.
pub fn encode_key(e: &KeyInput, mode: u32) -> Option<String> {
    let key = effective_key(e);
    let event_type: u8 = if e.event_type == "keyup" {
        3
    } else if e.repeat {
        2
    } else {
        1
    };
    let report_events = (mode & mode::REPORT_EVENT_TYPES) != 0;
    if event_type == 3 && !report_events {
        return None;
    }

    let disambiguate = (mode & mode::DISAMBIGUATE_KEYS) != 0;
    let report_alt = (mode & mode::REPORT_ALTERNATE_KEYS) != 0;
    let report_all = (mode & mode::REPORT_ALL_KEYS_AS_ESC) != 0;
    let report_text = (mode & mode::REPORT_ASSOCIATED_TEXT) != 0;
    let kitty_active = (mode & mode::KITTY_KEYBOARD_PROTOCOL) != 0;
    let event_suffix = if report_events && event_type != 1 {
        format!(":{event_type}")
    } else {
        String::new()
    };

    let mods = modifier(e);
    let app_cursor = (mode & mode::APP_CURSOR) != 0;

    if let Some(mod_code) = modifier_key_code(&key, &e.code, e.location) {
        if !report_all {
            return None;
        }
        return Some(format_kitty_u(&mod_code.to_string(), mods, &event_suffix, None));
    }

    if let Some(cursor) = cursor_key(&key) {
        if mods > 1 || !event_suffix.is_empty() {
            return Some(format!("{CSI}1;{mods}{event_suffix}{cursor}"));
        }
        return Some(format!("{}{cursor}", if app_cursor { SS3 } else { CSI }));
    }

    if let Some(tilde) = tilde_key(&key) {
        if mods > 1 || !event_suffix.is_empty() {
            return Some(format!("{CSI}{tilde};{mods}{event_suffix}~"));
        }
        return Some(format!("{CSI}{tilde}~"));
    }

    if let Some(fkey) = ss3_fkey(&key) {
        if mods > 1 || !event_suffix.is_empty() {
            return Some(format!("{CSI}1;{mods}{event_suffix}{fkey}"));
        }
        return Some(format!("{SS3}{fkey}"));
    }

    if let Some(functional) = kitty_functional_key(&key) {
        let is_basic = matches!(key.as_str(), "Enter" | "Tab" | "Backspace");
        if report_all
            || (!is_basic && (disambiguate || kitty_active))
            || (is_basic && (disambiguate || report_events) && (mods > 1 || !event_suffix.is_empty()))
        {
            return Some(format_kitty_u(&functional.to_string(), mods, &event_suffix, None));
        }
        if event_type == 3 {
            return None;
        }
    }

    let char_count = key.chars().count();
    if char_count == 1 {
        let ch = key.chars().next().unwrap();
        let is_alt_gr_text = e.ctrl
            && e.alt
            && e.code.is_empty()
            && control_char(&key).is_none()
            && !ch.is_ascii_alphabetic()
            && ch != ' ';
        if !is_alt_gr_text {
            let has_non_shift_mod = e.ctrl || e.alt || e.meta;
            let is_shift_space = e.shift && ch == ' ';
            if report_all
                || ((disambiguate || report_events)
                    && (has_non_shift_mod || is_shift_space || !event_suffix.is_empty()))
            {
                let (base, shifted) = base_and_shifted_code(&key, &e.code, e.shift);
                let key_param = match shifted {
                    Some(s) if report_alt => format!("{base}:{s}"),
                    _ => base.to_string(),
                };
                let text_code = (report_text && event_type != 3 && !has_non_shift_mod).then_some(ch as u32);
                return Some(format_kitty_u(&key_param, mods, &event_suffix, text_code));
            }
        }
    }

    if event_type == 3 {
        return None;
    }

    let alt = if e.alt { ESC } else { "" };
    match key.as_str() {
        "Enter" => {
            let nl = if (mode & mode::LINE_FEED_NEW_LINE) != 0 { "\r\n" } else { "\r" };
            return Some(format!("{alt}{nl}"));
        },
        "Backspace" => {
            let bs = if e.ctrl { "\x08" } else { "\x7f" };
            return Some(format!("{alt}{bs}"));
        },
        "Tab" => {
            if e.shift {
                return Some(format!("{CSI}Z"));
            }
            return Some(format!("{alt}\t"));
        },
        "Escape" => return Some(format!("{alt}{ESC}")),
        _ => {},
    }

    if e.meta {
        return None;
    }

    if e.ctrl && !e.alt {
        return control_char(&key).map(|c| c.to_string());
    }
    if e.ctrl && e.alt {
        let ch = key.chars().next()?;
        if key.chars().count() == 1 && (ch.is_ascii_alphabetic() || ch == ' ') {
            return control_char(&key).map(|c| format!("{ESC}{c}"));
        }
        return None;
    }
    if e.alt && char_count == 1 {
        return Some(format!("{ESC}{key}"));
    }

    None
}

/// Wrap pasted text for the PTY, honouring bracketed paste mode.
pub fn encode_paste(text: &str, mode: u32) -> String {
    if (mode & mode::BRACKETED_PASTE) != 0 {
        let sanitized = text.replace("\x1b[201~", "");
        format!("{CSI}200~{sanitized}{CSI}201~")
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseAction {
    Press,
    Release,
    Move,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct MouseReport {
    pub action: MouseAction,
    /// 0 left, 1 middle, 2 right; ignored for wheel/move-without-button.
    pub button: u8,
    /// Whether a button is held during `move`.
    pub held: bool,
    pub col: u16,
    pub row: u16,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Encode a mouse event for applications that enabled mouse reporting.
pub fn encode_mouse(m: &MouseReport, mode: u32) -> Option<String> {
    if (mode & mode::MOUSE_MODE) == 0 {
        return None;
    }
    if m.action == MouseAction::Move {
        let wants_motion = (mode & mode::MOUSE_MOTION) != 0 || ((mode & mode::MOUSE_DRAG) != 0 && m.held);
        if !wants_motion {
            return None;
        }
    }

    let mut code: u16 = match m.action {
        MouseAction::WheelUp => 64,
        MouseAction::WheelDown => 65,
        MouseAction::Move => 32 + if m.held { u16::from(m.button) } else { 3 },
        MouseAction::Press | MouseAction::Release => u16::from(m.button),
    };
    if m.shift {
        code += 4;
    }
    if m.alt {
        code += 8;
    }
    if m.ctrl {
        code += 16;
    }

    let x = m.col + 1;
    let y = m.row + 1;
    if (mode & mode::SGR_MOUSE) != 0 {
        let suffix = if m.action == MouseAction::Release { 'm' } else { 'M' };
        return Some(format!("{CSI}<{code};{x};{y}{suffix}"));
    }
    if m.action == MouseAction::Release {
        code = 3 + (code & !3);
    }
    if (mode & mode::UTF8_MOUSE) != 0 {
        if x > 2015 || y > 2015 {
            return None;
        }
        let b = char::from_u32(32 + u32::from(code))?;
        let cx = char::from_u32(32 + u32::from(x))?;
        let cy = char::from_u32(32 + u32::from(y))?;
        return Some(format!("{CSI}M{b}{cx}{cy}"));
    }
    if x > 223 || y > 223 {
        return None;
    }
    let mut out = Vec::with_capacity(6);
    out.extend_from_slice(b"\x1b[M");
    out.push((32 + code) as u8);
    out.push((32 + x) as u8);
    out.push((32 + y) as u8);
    // For coordinates > 95 (bytes 128..=255), encode as ISO-8859-1 / UTF-8 code points
    // matching JS String.fromCharCode.
    Some(out.into_iter().map(char::from).collect())
}

/// What a wheel movement does: report it to the program, send arrow keys, or scroll history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelAction {
    Report,
    Keys,
    History,
}

pub fn wheel_action(mode: u32, shift: bool) -> WheelAction {
    let reports_mouse = (mode & mode::MOUSE_MODE) != 0 && !shift;
    if reports_mouse {
        WheelAction::Report
    } else if (mode & mode::ALT_SCREEN) != 0 && (mode & mode::ALTERNATE_SCROLL) != 0 {
        WheelAction::Keys
    } else {
        WheelAction::History
    }
}

/// Arrow keys for `lines` of wheel movement on the alternate screen (negative is up).
pub fn wheel_keys(lines: i32, mode: u32) -> String {
    let key = if lines < 0 { 'A' } else { 'B' };
    let prefix = if (mode & mode::APP_CURSOR) != 0 { SS3 } else { CSI };
    let seq = format!("{prefix}{key}");
    seq.repeat(lines.unsigned_abs() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> KeyInput {
        KeyInput { key: k.into(), ..Default::default() }
    }

    #[test]
    fn encodes_kitty_and_legacy_keys() {
        let kitty = mode::DISAMBIGUATE_KEYS;
        let mut shift_enter = key("Enter");
        shift_enter.shift = true;
        assert_eq!(encode_key(&shift_enter, 0).as_deref(), Some("\r"));
        assert_eq!(encode_key(&shift_enter, kitty).as_deref(), Some("\x1b[13;2u"));

        let mut shift_tab_webkit = key("Unidentified");
        shift_tab_webkit.code = "Tab".into();
        shift_tab_webkit.shift = true;
        assert_eq!(encode_key(&shift_tab_webkit, 0).as_deref(), Some("\x1b[Z"));
        assert_eq!(encode_key(&shift_tab_webkit, kitty).as_deref(), Some("\x1b[9;2u"));

        let mut ctrl_c = key("c");
        ctrl_c.ctrl = true;
        assert_eq!(encode_key(&ctrl_c, 0).as_deref(), Some("\x03"));
        assert_eq!(encode_key(&ctrl_c, kitty).as_deref(), Some("\x1b[99;5u"));

        let report_all = mode::DISAMBIGUATE_KEYS | mode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(encode_key(&key("Enter"), report_all).as_deref(), Some("\x1b[13u"));
        assert_eq!(encode_key(&key("a"), report_all).as_deref(), Some("\x1b[97u"));
    }

    #[test]
    fn encodes_paste_and_mouse() {
        assert_eq!(encode_paste("a\nb\r\nc", 0), "a\rb\rc");
        assert_eq!(encode_paste("x\x1b[201~y", mode::BRACKETED_PASTE), "\x1b[200~xy\x1b[201~");

        let report = MouseReport {
            action: MouseAction::Press,
            button: 0,
            held: false,
            col: 4,
            row: 1,
            shift: false,
            alt: false,
            ctrl: false,
        };
        assert_eq!(encode_mouse(&report, 0), None);
        assert_eq!(
            encode_mouse(&report, mode::MOUSE_REPORT_CLICK | mode::SGR_MOUSE).as_deref(),
            Some("\x1b[<0;5;2M")
        );
    }
}

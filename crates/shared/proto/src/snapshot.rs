use crate::terminal::StreamSeq;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub anchor_state: AnchorState,
    pub bytes: ByteBuf,
    pub head_seq: StreamSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorState {
    pub cursor: (u16, u16),
    pub sgr: SgrAttrs,
    pub modes: TerminalModes,
    pub charset: CharsetState,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SgrAttrs {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub reverse: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    /// Standard 16-color palette index (0..=15).
    Palette(u8),
    /// 256-color extended palette (0..=255).
    Indexed(u8),
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct TerminalModes {
    pub deccmm_cursor_keys: bool,
    pub autowrap: bool,
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    pub mouse_reporting: MouseMode,
    pub origin_mode: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            deccmm_cursor_keys: false,
            autowrap: true,
            alt_screen: false,
            bracketed_paste: false,
            mouse_reporting: MouseMode::Off,
            origin_mode: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseMode {
    #[default]
    Off,
    X10,
    Normal,
    ButtonEvent,
    AnyEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharsetState {
    /// G0..G3 character set designations as raw final-bytes per ECMA-35.
    /// Default ('B','B','B','B') = US-ASCII.
    pub g: [u8; 4],
    /// Active GL set index (0..=3).
    pub gl: u8,
    /// Active GR set index (0..=3).
    pub gr: u8,
}

impl Default for CharsetState {
    fn default() -> Self {
        Self {
            g: [b'B'; 4],
            gl: 0,
            gr: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaSlice {
    pub bytes: ByteBuf,
    pub head_seq: StreamSeq,
}

#[must_use]
pub fn render_prefix_from_anchor(anchor: &AnchorState) -> String {
    let mut out = String::new();
    out.push_str("\x1b[0m");
    out.push_str("\x1b[?25h");

    apply_sgr(&mut out, &anchor.sgr);
    apply_modes(&mut out, anchor.modes);
    apply_charset(&mut out, anchor.charset);

    let row = anchor.cursor.1.saturating_add(1);
    let col = anchor.cursor.0.saturating_add(1);
    let _ = write!(out, "\x1b[{row};{col}H");

    out
}

fn apply_sgr(out: &mut String, sgr: &SgrAttrs) {
    let mut params = Vec::new();
    if sgr.bold {
        params.push("1".to_owned());
    }
    if sgr.faint {
        params.push("2".to_owned());
    }
    if sgr.italic {
        params.push("3".to_owned());
    }
    if sgr.underline {
        params.push("4".to_owned());
    }
    if sgr.blink {
        params.push("5".to_owned());
    }
    if sgr.reverse {
        params.push("7".to_owned());
    }
    if sgr.strikethrough {
        params.push("9".to_owned());
    }
    if let Some(fg) = sgr.fg {
        params.extend(color_params(fg, true));
    }
    if let Some(bg) = sgr.bg {
        params.extend(color_params(bg, false));
    }

    if !params.is_empty() {
        out.push_str("\x1b[");
        out.push_str(&params.join(";"));
        out.push('m');
    }
}

fn color_params(color: Color, foreground: bool) -> Vec<String> {
    match color {
        Color::Palette(index) => {
            let base = if foreground {
                if index < 8 {
                    30
                } else {
                    90
                }
            } else if index < 8 {
                40
            } else {
                100
            };
            vec![(base + i32::from(index % 8)).to_string()]
        }
        Color::Indexed(index) => vec![
            if foreground { "38" } else { "48" }.to_owned(),
            "5".to_owned(),
            index.to_string(),
        ],
        Color::Rgb(r, g, b) => vec![
            if foreground { "38" } else { "48" }.to_owned(),
            "2".to_owned(),
            r.to_string(),
            g.to_string(),
            b.to_string(),
        ],
    }
}

fn apply_modes(out: &mut String, modes: TerminalModes) {
    set_private_mode(out, 1, modes.deccmm_cursor_keys);
    set_private_mode(out, 6, modes.origin_mode);
    set_private_mode(out, 7, modes.autowrap);
    set_private_mode(out, 1000, !matches!(modes.mouse_reporting, MouseMode::Off));
    set_private_mode(
        out,
        1002,
        matches!(modes.mouse_reporting, MouseMode::ButtonEvent),
    );
    set_private_mode(
        out,
        1003,
        matches!(modes.mouse_reporting, MouseMode::AnyEvent),
    );
    set_private_mode(out, 1005, false);
    set_private_mode(out, 1006, !matches!(modes.mouse_reporting, MouseMode::Off));
    set_private_mode(out, 1049, modes.alt_screen);
    set_private_mode(out, 2004, modes.bracketed_paste);
}

fn set_private_mode(out: &mut String, mode: u16, enabled: bool) {
    out.push_str("\x1b[?");
    out.push_str(&mode.to_string());
    out.push(if enabled { 'h' } else { 'l' });
}

fn apply_charset(out: &mut String, charset: CharsetState) {
    for (index, set) in charset.g.iter().copied().enumerate() {
        let selector = match index {
            0 => '(',
            1 => ')',
            2 => '*',
            3 => '+',
            _ => continue,
        };
        out.push('\x1b');
        out.push(selector);
        out.push(char::from(set));
    }

    if charset.gl <= 3 {
        out.push('\x0f');
        if charset.gl != 0 {
            out.push('\x1b');
            out.push('n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_default_uses_ascii_designations() {
        let charset = CharsetState::default();

        assert_eq!(charset.g, [b'B'; 4]);
        assert_eq!(charset.gl, 0);
        assert_eq!(charset.gr, 0);
    }

    #[test]
    fn terminal_modes_default_matches_terminal_baseline() {
        let modes = TerminalModes::default();

        assert!(!modes.deccmm_cursor_keys);
        assert!(modes.autowrap);
        assert!(!modes.alt_screen);
        assert!(!modes.bracketed_paste);
        assert_eq!(modes.mouse_reporting, MouseMode::Off);
        assert!(!modes.origin_mode);
    }
}

use crate::terminal::StreamSeq;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub anchor_state: AnchorState,
    pub bytes: ByteBuf,
    pub head_seq: StreamSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalBaseline {
    pub cols: u16,
    pub rows: u16,
    pub anchor_state: AnchorState,
    pub head_seq: StreamSeq,
    pub byte_len: u32,
}

impl From<&Snapshot> for TerminalBaseline {
    fn from(snapshot: &Snapshot) -> Self {
        Self {
            cols: snapshot.cols,
            rows: snapshot.rows,
            anchor_state: snapshot.anchor_state.clone(),
            head_seq: snapshot.head_seq,
            byte_len: u32::try_from(snapshot.bytes.len()).unwrap_or(u32::MAX),
        }
    }
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

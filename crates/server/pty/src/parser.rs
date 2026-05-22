use cli_pocket_proto::{AnchorState, CharsetState, Color, MouseMode, SgrAttrs, TerminalModes};
use std::vec::Vec;
use vte::{Params, Parser, Perform};

pub struct AnchorTracker {
    parser: Parser,
    state: AnchorStateMut,
    at_safe_split: bool,
}

#[derive(Debug, Clone, Default)]
struct AnchorStateMut {
    cursor_row: u16,
    cursor_col: u16,
    sgr: SgrAttrs,
    modes: TerminalModes,
    charset: CharsetState,
    title: Option<String>,
}

impl AnchorTracker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            state: AnchorStateMut::default(),
            at_safe_split: true,
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.at_safe_split = false;

            let mut performer = TrackerPerform {
                state: &mut self.state,
                at_safe_split: &mut self.at_safe_split,
            };
            self.parser.advance(&mut performer, byte);
        }
    }

    #[must_use]
    pub fn is_at_safe_split(&self) -> bool {
        self.at_safe_split
    }

    #[must_use]
    pub fn snapshot_state(&self) -> AnchorState {
        AnchorState {
            cursor: (self.state.cursor_col, self.state.cursor_row),
            sgr: self.state.sgr,
            modes: self.state.modes,
            charset: self.state.charset,
            title: self.state.title.clone(),
        }
    }
}

impl Default for AnchorTracker {
    fn default() -> Self {
        Self::new()
    }
}

struct TrackerPerform<'a> {
    state: &'a mut AnchorStateMut,
    at_safe_split: &'a mut bool,
}

impl Perform for TrackerPerform<'_> {
    fn print(&mut self, c: char) {
        self.state.cursor_col = self.state.cursor_col.saturating_add(char_width(c));
        *self.at_safe_split = true;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.state.cursor_row = self.state.cursor_row.saturating_add(1);
                self.state.cursor_col = 0;
            }
            b'\r' => {
                self.state.cursor_col = 0;
            }
            b'\x08' => {
                self.state.cursor_col = self.state.cursor_col.saturating_sub(1);
            }
            b'\t' => {
                self.state.cursor_col = ((self.state.cursor_col / 8) + 1).saturating_mul(8);
            }
            _ => {}
        }

        *self.at_safe_split = true;
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        *self.at_safe_split = false;
    }

    fn put(&mut self, _byte: u8) {
        *self.at_safe_split = false;
    }

    fn unhook(&mut self) {
        *self.at_safe_split = true;
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(title) = parse_osc_title(params) {
            self.state.title = Some(title);
        }

        *self.at_safe_split = true;
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            'm' => self.sgr(params),
            'H' | 'f' => self.cursor_position(params),
            'A' => {
                self.state.cursor_row =
                    self.state.cursor_row.saturating_sub(first_param(params, 1));
            }
            'B' => {
                self.state.cursor_row =
                    self.state.cursor_row.saturating_add(first_param(params, 1));
            }
            'C' => {
                self.state.cursor_col =
                    self.state.cursor_col.saturating_add(first_param(params, 1));
            }
            'D' => {
                self.state.cursor_col =
                    self.state.cursor_col.saturating_sub(first_param(params, 1));
            }
            'E' => {
                self.state.cursor_row =
                    self.state.cursor_row.saturating_add(first_param(params, 1));
                self.state.cursor_col = 0;
            }
            'F' => {
                self.state.cursor_row =
                    self.state.cursor_row.saturating_sub(first_param(params, 1));
                self.state.cursor_col = 0;
            }
            'G' => {
                self.state.cursor_col = first_param(params, 1).saturating_sub(1);
            }
            'd' => {
                self.state.cursor_row = first_param(params, 1).saturating_sub(1);
            }
            'h' => self.mode_set(params, intermediates, true),
            'l' => self.mode_set(params, intermediates, false),
            _ => {}
        }

        *self.at_safe_split = true;
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if let Some(&marker) = intermediates.first() {
            if intermediates.len() == 1 {
                let idx = match marker {
                    b'(' => Some(0),
                    b')' => Some(1),
                    b'*' => Some(2),
                    b'+' => Some(3),
                    _ => None,
                };

                if let Some(idx) = idx {
                    self.state.charset.g[idx] = byte;
                    *self.at_safe_split = true;
                    return;
                }
            }
        }

        *self.at_safe_split = true;
    }
}

impl TrackerPerform<'_> {
    fn sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.state.sgr = SgrAttrs::default();
            return;
        }

        let items: Vec<&[u16]> = params.iter().collect();
        let mut index = 0;
        while index < items.len() {
            let code = items[index].first().copied().unwrap_or(0);
            match code {
                0 => self.state.sgr = SgrAttrs::default(),
                1 => self.state.sgr.bold = true,
                2 => self.state.sgr.faint = true,
                3 => self.state.sgr.italic = true,
                4 => self.state.sgr.underline = true,
                5 => self.state.sgr.blink = true,
                7 => self.state.sgr.reverse = true,
                9 => self.state.sgr.strikethrough = true,
                21 | 22 => {
                    self.state.sgr.bold = false;
                    self.state.sgr.faint = false;
                }
                23 => self.state.sgr.italic = false,
                24 => self.state.sgr.underline = false,
                25 => self.state.sgr.blink = false,
                27 => self.state.sgr.reverse = false,
                29 => self.state.sgr.strikethrough = false,
                30..=37 => self.state.sgr.fg = palette_color(code - 30),
                39 => self.state.sgr.fg = None,
                40..=47 => self.state.sgr.bg = palette_color(code - 40),
                49 => self.state.sgr.bg = None,
                90..=97 => self.state.sgr.fg = palette_color(code - 82),
                100..=107 => self.state.sgr.bg = palette_color(code - 92),
                38 | 48 => {
                    if let Some((color, consumed)) = parse_extended_color(&items, index) {
                        if code == 38 {
                            self.state.sgr.fg = Some(color);
                        } else {
                            self.state.sgr.bg = Some(color);
                        }
                        index = index.saturating_add(consumed.saturating_sub(1));
                    }
                }
                _ => {}
            }

            index += 1;
        }
    }

    fn cursor_position(&mut self, params: &Params) {
        self.state.cursor_row = first_param(params, 1).saturating_sub(1);
        self.state.cursor_col = second_param(params, 1).saturating_sub(1);
    }

    fn mode_set(&mut self, params: &Params, intermediates: &[u8], on: bool) {
        if intermediates != b"?" {
            return;
        }

        for param in params {
            match param.first().copied().unwrap_or(0) {
                1 => self.state.modes.deccmm_cursor_keys = on,
                7 => self.state.modes.autowrap = on,
                1049 => self.state.modes.alt_screen = on,
                2004 => self.state.modes.bracketed_paste = on,
                9 => {
                    self.state.modes.mouse_reporting =
                        if on { MouseMode::X10 } else { MouseMode::Off }
                }
                1000 => {
                    self.state.modes.mouse_reporting = if on {
                        MouseMode::Normal
                    } else {
                        MouseMode::Off
                    }
                }
                1002 => {
                    self.state.modes.mouse_reporting = if on {
                        MouseMode::ButtonEvent
                    } else {
                        MouseMode::Off
                    }
                }
                1003 => {
                    self.state.modes.mouse_reporting = if on {
                        MouseMode::AnyEvent
                    } else {
                        MouseMode::Off
                    }
                }
                6 => self.state.modes.origin_mode = on,
                _ => {}
            }
        }
    }
}

fn first_param(params: &Params, default: u16) -> u16 {
    params
        .iter()
        .next()
        .and_then(|param| param.first().copied())
        .filter(|&value| value != 0)
        .unwrap_or(default)
}

fn second_param(params: &Params, default: u16) -> u16 {
    params
        .iter()
        .nth(1)
        .and_then(|param| param.first().copied())
        .filter(|&value| value != 0)
        .unwrap_or(default)
}

fn palette_color(value: u16) -> Option<Color> {
    u8::try_from(value).ok().map(Color::Palette)
}

fn parse_osc_title(params: &[&[u8]]) -> Option<String> {
    let (head, tail) = params.split_first()?;
    if *head != b"0" && *head != b"2" {
        return None;
    }

    let mut raw = Vec::new();
    for (index, part) in tail.iter().enumerate() {
        if index != 0 {
            raw.push(b';');
        }
        raw.extend_from_slice(part);
    }

    String::from_utf8(raw).ok()
}

fn parse_extended_color(items: &[&[u16]], index: usize) -> Option<(Color, usize)> {
    let current = *items.get(index)?;
    match current.get(1).copied() {
        Some(5) => {
            let value = current
                .get(2)
                .copied()
                .and_then(|value| u8::try_from(value).ok())?;
            Some((Color::Indexed(value), 1))
        }
        Some(2) => {
            let r = current
                .get(2)
                .copied()
                .and_then(|value| u8::try_from(value).ok())?;
            let g = current
                .get(3)
                .copied()
                .and_then(|value| u8::try_from(value).ok())?;
            let b = current
                .get(4)
                .copied()
                .and_then(|value| u8::try_from(value).ok())?;
            Some((Color::Rgb(r, g, b), 1))
        }
        None => match items.get(index + 1)?.first().copied()? {
            5 => {
                let value = items
                    .get(index + 2)?
                    .first()
                    .copied()
                    .and_then(|value| u8::try_from(value).ok())?;
                Some((Color::Indexed(value), 3))
            }
            2 => {
                let r = items
                    .get(index + 2)?
                    .first()
                    .copied()
                    .and_then(|value| u8::try_from(value).ok())?;
                let g = items
                    .get(index + 3)?
                    .first()
                    .copied()
                    .and_then(|value| u8::try_from(value).ok())?;
                let b = items
                    .get(index + 4)?
                    .first()
                    .copied()
                    .and_then(|value| u8::try_from(value).ok())?;
                Some((Color::Rgb(r, g, b), 5))
            }
            _ => None,
        },
        _ => None,
    }
}

fn char_width(c: char) -> u16 {
    u16::from(!c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_advances_cursor() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"hello");

        assert_eq!(tracker.snapshot_state().cursor, (5, 0));
    }

    #[test]
    fn newline_resets_column() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"hi\nworld");

        assert_eq!(tracker.snapshot_state().cursor, (5, 1));
    }

    #[test]
    fn sgr_bold_red() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"\x1b[1;31m");

        let state = tracker.snapshot_state();
        assert!(state.sgr.bold);
        assert_eq!(state.sgr.fg, Some(Color::Palette(1)));
    }

    #[test]
    fn sgr_reset() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"\x1b[1;31m\x1b[0m");

        let state = tracker.snapshot_state();
        assert!(!state.sgr.bold);
        assert_eq!(state.sgr.fg, None);
    }

    #[test]
    fn alt_screen_enter_exit() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"\x1b[?1049h");

        assert!(tracker.snapshot_state().modes.alt_screen);

        tracker.advance(b"\x1b[?1049l");

        assert!(!tracker.snapshot_state().modes.alt_screen);
    }

    #[test]
    fn osc_window_title() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"\x1b]0;hello title\x07");

        assert_eq!(
            tracker.snapshot_state().title.as_deref(),
            Some("hello title")
        );
    }

    #[test]
    fn extended_sgr_colors_do_not_leak_params() {
        let mut tracker = AnchorTracker::new();
        tracker.advance(b"\x1b[38;2;1;2;3m");

        let state = tracker.snapshot_state();
        assert_eq!(state.sgr.fg, Some(Color::Rgb(1, 2, 3)));
        assert!(!state.sgr.faint);

        tracker.advance(b"\x1b[48:5:123m");

        assert_eq!(tracker.snapshot_state().sgr.bg, Some(Color::Indexed(123)));
    }
}

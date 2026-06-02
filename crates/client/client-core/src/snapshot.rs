use std::fmt::Write as _;

use bytes::Bytes;
use cli_pocket_proto::{
    AnchorState, CharsetState, Color, MouseMode, SgrAttrs, StreamSeq, TerminalInfo, TerminalModes,
};

#[derive(Debug, Clone)]
pub struct TerminalSnapshot {
    pub info: TerminalInfo,
    pub start_seq: StreamSeq,
    pub end_seq: StreamSeq,
    pub bytes: Bytes,
    pub render_prefix: String,
}

impl TerminalSnapshot {
    #[must_use]
    pub fn new(
        info: TerminalInfo,
        start_seq: StreamSeq,
        end_seq: StreamSeq,
        bytes: Bytes,
        render_prefix: String,
    ) -> Self {
        Self {
            info,
            start_seq,
            end_seq,
            bytes,
            render_prefix,
        }
    }
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

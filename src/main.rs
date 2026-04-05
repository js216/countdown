mod font_vga16;
mod raster;
mod term;

use std::time::Instant;

use font_vga16::FONT;
use term::Key;

// ── Duration parsing ──────────────────────────────────────────────────────

fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Pure number → minutes
    if let Ok(n) = s.parse::<u64>() {
        return Some(n * 60);
    }
    // Sequences like 1h30m, 90s, 30m
    let mut total: u64 = 0;
    let mut num_buf = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
        } else {
            let n: u64 = num_buf.parse().ok()?;
            num_buf.clear();
            match ch {
                'h' | 'H' => total += n * 3600,
                'm' | 'M' => total += n * 60,
                's' | 'S' => total += n,
                _ => return None,
            }
        }
    }
    // Trailing digits without unit → minutes
    if !num_buf.is_empty() {
        let n: u64 = num_buf.parse().ok()?;
        total += n * 60;
    }
    if total == 0 { None } else { Some(total) }
}

fn format_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────

fn render(
    text: &str,
    cols: usize,
    rows: usize,
    paused: bool,
    finished: bool,
    remaining_ms: u64,
    input_buf: &str,
    flash_on: bool,
) {
    let text_bytes: Vec<u8> = text.bytes().collect();
    let num_chars = text_bytes.len();
    let fw = FONT.font_w;
    let fh = FONT.font_h;
    let text_px_w = num_chars * fw;

    // half-blocks: each font pixel = scale columns wide, 2 pixel-rows per terminal row
    let avail_cols = cols.saturating_sub(4);
    let avail_rows = rows.saturating_sub(4);
    let scale_x = if text_px_w > 0 { avail_cols / text_px_w } else { 1 };
    let scale_y = if fh > 0 { avail_rows * 2 / fh } else { 1 };
    let scale = scale_x.min(scale_y).max(1);

    let disp_cols = text_px_w * scale;
    let disp_rows = fh * scale / 2;

    let margin_left = cols.saturating_sub(disp_cols) / 2;
    let margin_top = rows.saturating_sub(disp_rows + 2) / 2;

    let remaining_secs = (remaining_ms + 999) / 1000;
    let color = if finished {
        if flash_on { "\x1b[97m" } else { "\x1b[91m" }
    } else if paused {
        "\x1b[36m"
    } else if remaining_secs <= 10 {
        "\x1b[91m"
    } else if remaining_secs <= 60 {
        "\x1b[93m"
    } else {
        "\x1b[92m"
    };

    let mut buf = String::with_capacity(cols * rows * 6);
    buf.push_str("\x1b[H");

    for tr in 0..rows {
        if tr >= margin_top && tr < margin_top + disp_rows {
            let dy = tr - margin_top;
            buf.push_str("\x1b[0m");
            for _ in 0..margin_left {
                buf.push(' ');
            }
            buf.push_str(color);
            for tc in 0..disp_cols {
                let top_py = dy * 2;
                let bot_py = dy * 2 + 1;

                let char_idx = tc / (fw * scale);
                let font_px = (tc / scale) % fw;
                let font_py_top = top_py / scale;
                let font_py_bot = bot_py / scale;

                if char_idx >= num_chars {
                    buf.push(' ');
                    continue;
                }

                let ch = text_bytes[char_idx];
                let top = FONT.pixel(ch, font_px, font_py_top);
                let bot = FONT.pixel(ch, font_px, font_py_bot);

                buf.push(match (top, bot) {
                    (true, true) => '\u{2588}',
                    (true, false) => '\u{2580}',
                    (false, true) => '\u{2584}',
                    (false, false) => ' ',
                });
            }
            buf.push_str("\x1b[0m");
            let used = margin_left + disp_cols;
            for _ in used..cols {
                buf.push(' ');
            }
        } else if tr == rows.saturating_sub(2) {
            buf.push_str("\x1b[0m");
            if !input_buf.is_empty() {
                let msg = format!("  New time: {input_buf}_");
                buf.push_str("\x1b[93m");
                buf.push_str(&msg);
                buf.push_str("\x1b[0m");
                for _ in msg.len()..cols {
                    buf.push(' ');
                }
            } else {
                for _ in 0..cols {
                    buf.push(' ');
                }
            }
        } else if tr == rows.saturating_sub(1) {
            buf.push_str("\x1b[0m\x1b[2m");
            let status = if finished {
                "TIME'S UP!  [R] reset  [Q] quit  [0-9] new time"
            } else if paused {
                "PAUSED  [SPACE] resume  [R] reset  [Q] quit  [0-9] new time"
            } else {
                "[SPACE] pause  [R] reset  [Q] quit  [0-9] new time"
            };
            let pad_l = cols.saturating_sub(status.len()) / 2;
            for _ in 0..pad_l {
                buf.push(' ');
            }
            buf.push_str(status);
            let used = pad_l + status.len();
            for _ in used..cols {
                buf.push(' ');
            }
            buf.push_str("\x1b[0m");
        } else {
            buf.push_str("\x1b[0m");
            for _ in 0..cols {
                buf.push(' ');
            }
        }
        if tr + 1 < rows {
            buf.push_str("\r\n");
        }
    }

    term::write_bytes(buf.as_bytes());
}

// ── Bell ──────────────────────────────────────────────────────────────────

fn find_bell_wav() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("bell.wav");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = std::path::PathBuf::from("bell.wav");
    if p.exists() {
        return Some(p);
    }
    None
}

fn play_bell() {
    if let Some(path) = find_bell_wav() {
        let path_s = path.to_string_lossy().to_string();
        for cmd in &["aplay", "paplay", "pw-play"] {
            if std::process::Command::new(cmd)
                .arg(&path_s)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
            {
                return;
            }
        }
    }
    term::write_bytes(b"\x07");
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "30m".to_string());
    let mut total_secs = match parse_duration(&arg) {
        Some(s) => s,
        None => {
            eprintln!("Usage: countdown [DURATION]  e.g. 30m, 1h30m, 90s, 45");
            std::process::exit(1);
        }
    };

    let _raw = term::RawMode::enter();

    let mut remaining_ms: u64 = total_secs * 1000;
    let mut paused = false;
    let mut finished = false;
    let mut input_buf = String::new();
    let mut last_tick = Instant::now();
    let mut bell_played = false;
    let mut prev_display = String::new();
    let mut needs_redraw = true;

    loop {
        let (cols, rows) = term::get_size();

        if term::take_resized() {
            needs_redraw = true;
        }

        let now = Instant::now();
        if !paused && !finished {
            let elapsed = now.duration_since(last_tick).as_millis() as u64;
            if elapsed >= remaining_ms {
                remaining_ms = 0;
                finished = true;
                needs_redraw = true;
            } else {
                remaining_ms -= elapsed;
            }
        }
        last_tick = now;

        let display_secs = if remaining_ms == 0 {
            0
        } else {
            ((remaining_ms - 1) / 1000) + 1
        };
        let display_str = format_time(display_secs);

        if display_str != prev_display {
            needs_redraw = true;
            prev_display.clone_from(&display_str);
        }

        let flash_on = if finished {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            (ms / 500) % 2 == 0
        } else {
            false
        };

        if finished {
            needs_redraw = true;
        }

        if needs_redraw {
            render(
                &display_str, cols, rows, paused, finished, remaining_ms, &input_buf, flash_on,
            );
            needs_redraw = false;
        }

        if finished && !bell_played {
            play_bell();
            bell_played = true;
        }

        if let Some(key) = term::poll_key(50) {
            match key {
                Key::CtrlC => break,
                Key::Escape => {
                    if input_buf.is_empty() {
                        break;
                    }
                    input_buf.clear();
                    needs_redraw = true;
                }
                Key::Char(b'q' | b'Q' | b'x' | b'X') if input_buf.is_empty() => break,
                Key::Char(b' ') if input_buf.is_empty() => {
                    if !finished {
                        paused = !paused;
                        last_tick = Instant::now();
                        needs_redraw = true;
                    }
                }
                Key::Char(b'r' | b'R') if input_buf.is_empty() => {
                    remaining_ms = total_secs * 1000;
                    paused = false;
                    finished = false;
                    bell_played = false;
                    last_tick = Instant::now();
                    needs_redraw = true;
                }
                Key::Char(b @ b'0'..=b'9') => {
                    input_buf.push(b as char);
                    needs_redraw = true;
                }
                Key::Char(b @ (b's' | b'S' | b'm' | b'M' | b'h' | b'H'))
                    if !input_buf.is_empty() =>
                {
                    input_buf.push(b.to_ascii_lowercase() as char);
                    needs_redraw = true;
                }
                Key::Enter => {
                    if !input_buf.is_empty() {
                        if let Some(secs) = parse_duration(&input_buf) {
                            total_secs = secs;
                            remaining_ms = secs * 1000;
                            paused = false;
                            finished = false;
                            bell_played = false;
                            last_tick = Instant::now();
                        }
                        input_buf.clear();
                        needs_redraw = true;
                    }
                }
                Key::Backspace => {
                    if !input_buf.is_empty() {
                        input_buf.pop();
                        needs_redraw = true;
                    }
                }
                _ => {}
            }
        }
    }
}

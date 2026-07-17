//! ANSI frame renderer reproducing the begin2 console layout: scrolling
//! chart/message region on the left, position display and SYSTEMS STATUS
//! panels fixed on the right (see original1-4.png).

use begin_core::math::norm360;
use begin_core::object::{Kind, ObjId, ShieldState};
use begin_core::Game;

pub const SCREEN_W: usize = 80;
pub const SCREEN_H: usize = 24;
pub const LEFT_W: usize = 50; // scroll region width
const BOX_X: usize = 51; // right panels start (0-based)
const BOX_W: usize = 29;

// ANSI helpers
pub const RESET: &str = "\x1b[0m";
pub const GREEN: &str = "\x1b[32m";
pub const BGREEN: &str = "\x1b[92m";
pub const RED: &str = "\x1b[91m";
pub const CYAN: &str = "\x1b[96m";
pub const YELLOW: &str = "\x1b[93m";
pub const BROWN: &str = "\x1b[33m";
pub const WHITE: &str = "\x1b[97m";
pub const GREY: &str = "\x1b[37m";
pub const DIM: &str = "\x1b[90m";
// blinking variants (begin2 flashes damage X's, torpedoes, destruct drama)
pub const RBLINK: &str = "\x1b[91;5m";
pub const YBLINK: &str = "\x1b[93;5m";

/// Printable width of a string containing ANSI escape sequences.
pub fn printable_width(s: &str) -> usize {
    let mut w = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            w += 1;
        }
    }
    w
}

/// A left-region line with embedded ANSI plus its printable length.
#[derive(Clone)]
pub struct Line {
    pub text: String,
    pub width: usize,
}

impl Line {
    pub fn new(text: String, width: usize) -> Line {
        Line { text, width }
    }
    pub fn plain(s: &str) -> Line {
        Line { text: s.to_string(), width: s.chars().count() }
    }
}

/// Word-wrap a line with embedded ANSI at `max` printable columns, carrying
/// the active SGR state onto continuation lines (indented two spaces) so
/// long crew reports never spill into the right-hand panels.
fn wrap_ansi(text: &str, max: usize) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    let mut state = String::new(); // active SGR sequences since last reset
    // last breakable space: (byte pos in cur, printable width there, state)
    let mut brk: Option<(usize, usize, String)> = None;
    let mut chars = text.chars().peekable();
    loop {
        let Some(c) = chars.next() else { break };
        if c == '\x1b' {
            let mut seq = String::new();
            seq.push(c);
            while let Some(&n) = chars.peek() {
                seq.push(n);
                chars.next();
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
            if seq == RESET {
                state.clear();
            } else {
                state.push_str(&seq);
            }
            cur.push_str(&seq);
            continue;
        }
        if w >= max {
            // split: prefer the last space seen on this segment
            let (head, head_w, carry_state, tail) = match brk.take() {
                Some((pos, bw, st)) => {
                    let tail = cur[pos + 1..].to_string();
                    cur.truncate(pos);
                    (std::mem::take(&mut cur), bw, st, tail)
                }
                None => (std::mem::take(&mut cur), w, state.clone(), String::new()),
            };
            out.push(Line { text: format!("{head}{RESET}"), width: head_w });
            let tail_w = printable_width(&tail);
            cur = format!("{carry_state}  {tail}");
            w = 2 + tail_w;
        }
        if c == ' ' {
            brk = Some((cur.len(), w, state.clone()));
        }
        cur.push(c);
        w += 1;
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(Line { text: cur, width: w });
    }
    out
}

/// Per-viewer display state: the scroll buffer and scanner magnification.
pub struct Display {
    pub scroll: Vec<Line>,
    pub scan_range: f64,
    /// Render brief weapon-fire/detonation flash frames (`flash off` disables).
    pub flash: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display { scroll: Vec::new(), scan_range: 24000.0, flash: true }
    }
}

impl Display {
    pub fn push(&mut self, l: Line) {
        // measure from the text itself (callers' widths are approximate) and
        // wrap so nothing bleeds into the right-hand panels
        for seg in wrap_ansi(&l.text, LEFT_W) {
            self.scroll.push(seg);
        }
        if self.scroll.len() > 400 {
            self.scroll.drain(0..100);
        }
    }
    pub fn push_plain(&mut self, s: &str) {
        self.push(Line::plain(s));
    }
}

fn class_color(g: &Game, viewer_side: usize, id: ObjId, is_viewer: bool) -> &'static str {
    if is_viewer {
        WHITE
    } else if g.obj(id).nation == viewer_side {
        CYAN
    } else {
        RED
    }
}

/// The chart block (manual §VI CHART): one line per ship, viewer first.
pub fn chart_lines(g: &Game, viewer: ObjId, out: &mut Vec<Line>) {
    let side = g.obj(viewer).nation;
    out.push(Line::new(
        format!("{WHITE}             WARP COURSE BEARING RANGE  MARK CLASS{RESET}"),
        50,
    ));
    let mut ids: Vec<ObjId> = g.ship_ids();
    ids.retain(|&i| {
        i == viewer || !g.fog || g.obj(i).contact(side).ever
    });
    ids.sort_by_key(|&i| if i == viewer { 0 } else { 1 });
    for id in ids {
        let o = g.obj(id);
        // fog ghosts: show last-known data
        let c = o.contact(side);
        let (warp, course, mark3d) = if !g.fog || c.visible || id == viewer {
            (o.warp, o.course, o.mark)
        } else {
            (c.last_warp, c.last_course, c.last_mark)
        };
        let name_col = format!("{:>11}:", truncate(&o.name, 11));
        let color = class_color(g, side, id, id == viewer);
        // warp arrow: accelerating/decelerating
        let arrow = if (o.desired_warp - o.warp).abs() < 0.05 {
            ' '
        } else if o.desired_warp > o.warp {
            '\u{2191}'
        } else {
            '\u{2193}'
        };
        let course_str = if mark3d.abs() >= 0.5 {
            format!("{:.0}^{:.0}", course, mark3d)
        } else {
            format!("{:.0}\u{b0}", course)
        };
        if id == viewer {
            let helm = match o.helm {
                begin_core::object::HelmMode::Course => String::new(),
                begin_core::object::HelmMode::Pursue => "Pursuing".into(),
                begin_core::object::HelmMode::Elude => "Eluding".into(),
            };
            let t = format!(
                "{color}{name_col}{RESET}{GREEN} {:4.1}{arrow} {:>6} {:>8}{RESET}",
                warp, course_str, helm
            );
            out.push(Line::new(t, 12 + 22));
        } else {
            let d = begin_core::systems::helm::apparent_dist(g, viewer, id, side);
            let (bearing, _bm) =
                begin_core::systems::helm::target_bearing_mark(g, viewer, id, side);
            let bearing = if bearing.round() >= 360.0 { 0.0 } else { bearing };
            let rel = norm360(bearing - g.obj(viewer).course);
            let rel = if rel.round() >= 360.0 { 0.0 } else { rel };
            let class = &g.data.ships[o.ship.as_ref().unwrap().design].abbrev;
            let t = format!(
                "{color}{name_col}{RESET}{GREEN} {:4.1}{arrow} {:>6} {:6.0}\u{b0} {:6.0} {:4.0}\u{b0}  {}{}{}{RESET}",
                warp,
                course_str,
                bearing,
                d,
                rel,
                color,
                class,
                GREEN
            );
            out.push(Line::new(t, 50));
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

/// Render the full frame for a viewer. `flashes` (usually empty) overlays
/// transient weapon effects on the scope — used for the brief flash frame.
pub fn render(
    g: &Game,
    viewer: Option<ObjId>,
    disp: &Display,
    prompt_name: &str,
    flashes: &[begin_core::events::Flash],
) -> String {
    let mut grid: Vec<Vec<(char, &'static str)>> =
        vec![vec![(' ', GREEN); SCREEN_W]; SCREEN_H - 1];

    // ---- right panels
    if let Some(v) = viewer.filter(|&v| g.get(v).is_some()) {
        draw_scope(g, v, disp, &mut grid);
        if !flashes.is_empty() {
            draw_flashes(g, v, disp, flashes, &mut grid);
        }
        draw_status_panel(g, v, &mut grid);
    }

    // ---- compose: left scroll region overlaid on the grid rows
    let visible = SCREEN_H - 2;
    let start = disp.scroll.len().saturating_sub(visible);
    let lines = &disp.scroll[start..];

    let mut out = String::with_capacity(8192);
    out.push_str("\x1b[2J\x1b[H"); // clear + home
    for row in 0..SCREEN_H - 1 {
        // left region: scroll line if present
        if row < lines.len() {
            out.push_str(&lines[row].text);
            // pad to LEFT_W using printable width
            let pad = LEFT_W.saturating_sub(lines[row].width.min(LEFT_W));
            for _ in 0..pad {
                out.push(' ');
            }
        } else {
            out.push_str(&" ".repeat(LEFT_W));
        }
        // right region from the grid (reset between color changes so a
        // blinking cell doesn't leak the blink attribute into neighbours)
        let mut cur: &str = "";
        for col in BOX_X..SCREEN_W {
            let (ch, color) = grid[row][col];
            if color != cur {
                out.push_str(RESET);
                out.push_str(color);
                cur = color;
            }
            out.push(ch);
        }
        out.push_str(RESET);
        out.push_str("\r\n");
    }
    // prompt line
    out.push_str(&format!("{CYAN}{prompt_name}{RESET}: "));
    out
}

fn put(grid: &mut [Vec<(char, &'static str)>], row: usize, col: usize, ch: char, color: &'static str) {
    if row < grid.len() && col < SCREEN_W {
        grid[row][col] = (ch, color);
    }
}

fn put_str(grid: &mut [Vec<(char, &'static str)>], row: usize, col: usize, s: &str, color: &'static str) {
    for (i, ch) in s.chars().enumerate() {
        put(grid, row, col + i, ch, color);
    }
}

fn draw_box(grid: &mut [Vec<(char, &'static str)>], r0: usize, r1: usize) {
    let color = BROWN;
    for col in BOX_X..BOX_X + BOX_W {
        put(grid, r0, col, '\u{2550}', color);
        put(grid, r1, col, '\u{2550}', color);
    }
    for row in r0..=r1 {
        put(grid, row, BOX_X, '\u{2551}', color);
        put(grid, row, BOX_X + BOX_W - 1, '\u{2551}', color);
    }
    put(grid, r0, BOX_X, '\u{2554}', color);
    put(grid, r0, BOX_X + BOX_W - 1, '\u{2557}', color);
    put(grid, r1, BOX_X, '\u{255a}', color);
    put(grid, r1, BOX_X + BOX_W - 1, '\u{255d}', color);
}

/// Position display (upper right): objects around the viewer, 2-letter ship
/// tags, ordnance dots, dim reference grid, viewer as a diamond.
fn draw_scope(g: &Game, viewer: ObjId, disp: &Display, grid: &mut [Vec<(char, &'static str)>]) {
    let (r0, r1) = (0usize, 13usize);
    draw_box(grid, r0, r1);
    let iw = BOX_W - 2; // 27
    let ih = r1 - r0 - 1; // 12
    // dim reference dots
    for row in 1..=ih {
        for col in 0..iw {
            if col % 6 == 2 && row % 3 == 1 {
                put(grid, r0 + row, BOX_X + 1 + col, '\u{b7}', DIM);
            }
        }
    }
    let center = g.obj(viewer).pos;
    let side = g.obj(viewer).nation;
    let range = disp.scan_range.max(100.0);
    let half_w = (iw / 2) as f64;
    let half_h = (ih / 2) as f64;
    for id in g.ids() {
        if id == viewer {
            continue;
        }
        let o = g.obj(id);
        if g.fog && o.nation != side && !o.contact(side).visible && !o.contact(side).ever {
            continue;
        }
        let pos = begin_core::systems::helm::apparent_pos(g, id, side);
        let dx = pos.x - center.x;
        let dy = pos.y - center.y;
        if dx.abs() > range || dy.abs() > range {
            continue;
        }
        let col = ((dx / range) * half_w).round() as isize + half_w as isize;
        let row = (-(dy / range) * half_h).round() as isize + half_h as isize + 1;
        if col < 0 || col >= iw as isize || row < 1 || row > ih as isize {
            continue;
        }
        let (col, row) = (BOX_X + 1 + col as usize, r0 + row as usize);
        let hostile = o.nation != side;
        let stale = g.fog && hostile && !o.contact(side).visible;
        match o.kind {
            Kind::Ship => {
                let tag: String = o.name.chars().take(2).collect();
                let color = if stale { GREY } else if hostile { RED } else { YELLOW };
                put_str(grid, row, col.min(SCREEN_W - 2), &tag, color);
            }
            Kind::Torp => {
                // begin2 draws torpedoes as a flashing two-dot streak
                let color = if hostile { RBLINK } else { YBLINK };
                put(grid, row, col, '\u{b7}', color);
                put(grid, row, (col + 1).min(BOX_X + iw), '\u{b7}', color);
            }
            Kind::Probe => {
                put(grid, row, col, 'o', if hostile { RED } else { YELLOW });
            }
        }
    }
    // environment bodies on the scope
    for b in &g.env.bodies {
        let dx = b.pos.x - center.x;
        let dy = b.pos.y - center.y;
        if dx.abs() > range || dy.abs() > range {
            continue;
        }
        let col = ((dx / range) * half_w).round() as isize + half_w as isize;
        let row = (-(dy / range) * half_h).round() as isize + half_h as isize + 1;
        if col >= 0 && (col as usize) < iw && row >= 1 && row <= ih as isize {
            put(grid, r0 + row as usize, BOX_X + 1 + col as usize, '\u{25cf}', BROWN);
        }
    }
    // viewer diamond at center
    put(grid, r0 + 1 + ih / 2, BOX_X + 1 + iw / 2, '\u{25c6}', YELLOW);
    // scanning range caption
    let caption = format!("Scanning range: {:>6.0}", disp.scan_range);
    put_str(grid, r1 - 1, BOX_X + 2, &caption, GREEN);
}

/// Overlay transient weapon effects on the scope: phaser/rail beams as a
/// line of stars, detonations as a blast disc. Drawn only on the brief
/// flash frame between cycles (`flash off` disables it).
fn draw_flashes(
    g: &Game,
    viewer: ObjId,
    disp: &Display,
    flashes: &[begin_core::events::Flash],
    grid: &mut [Vec<(char, &'static str)>],
) {
    use begin_core::events::Flash;
    let (r0, r1) = (0usize, 13usize);
    let iw = BOX_W - 2;
    let ih = r1 - r0 - 1;
    let center = g.obj(viewer).pos;
    let range = disp.scan_range.max(100.0);
    let half_w = (iw / 2) as f64;
    let half_h = (ih / 2) as f64;
    let cell = |dx: f64, dy: f64| -> Option<(usize, usize)> {
        if dx.abs() > range || dy.abs() > range {
            return None;
        }
        let col = ((dx / range) * half_w).round() as isize + half_w as isize;
        let row = (-(dy / range) * half_h).round() as isize + half_h as isize + 1;
        if col < 0 || col >= iw as isize || row < 1 || row > ih as isize {
            return None;
        }
        Some((r0 + row as usize, BOX_X + 1 + col as usize))
    };
    for f in flashes {
        match *f {
            Flash::Beam { from, to } => {
                let d = to - from;
                for s in 1..=48 {
                    let t = s as f64 / 48.0;
                    let p = from + d * t;
                    if let Some((row, col)) = cell(p.x - center.x, p.y - center.y) {
                        put(grid, row, col, '*', WHITE);
                    }
                }
            }
            Flash::Blast { pos, radius } => {
                for row in 1..=ih {
                    for col in 0..iw {
                        let dx = (col as f64 - half_w) / half_w * range;
                        let dy = -((row as f64 - 1.0 - half_h) / half_h) * range;
                        let world = begin_core::math::Vec3::new(
                            center.x + dx - pos.x,
                            center.y + dy - pos.y,
                            0.0,
                        );
                        if world.len() < radius {
                            put(grid, r0 + row, BOX_X + 1 + col, '*', RBLINK);
                        }
                    }
                }
                if let Some((row, col)) = cell(pos.x - center.x, pos.y - center.y) {
                    put(grid, row, col, '*', YBLINK);
                }
            }
        }
    }
}

/// SYSTEMS STATUS panel (lower right).
fn draw_status_panel(g: &Game, viewer: ObjId, grid: &mut [Vec<(char, &'static str)>]) {
    let (r0, r1) = (15usize, SCREEN_H - 2);
    draw_box(grid, r0, r1);
    put_str(grid, r0 + 1, BOX_X + 7, "SYSTEMS STATUS", RED);
    let s = g.obj(viewer).ship.as_ref().unwrap();
    let d = &g.data.ships[s.design];

    // Banks: . - = ≡ charge stages; x damaged
    let mut banks = String::new();
    for b in &s.banks {
        banks.push(if b.sys.destroyed() {
            'x'
        } else {
            let f = b.charge / d.banks_charge.max(0.001);
            if f >= 1.0 {
                '\u{2261}'
            } else if f >= 0.66 {
                '='
            } else if f >= 0.33 {
                '-'
            } else {
                '.'
            }
        });
    }
    // Tubes: o loaded/ready
    let mut tubes = String::new();
    for t in &s.tubes {
        tubes.push(if t.sys.destroyed() {
            'x'
        } else if t.loaded.is_some() && t.charge >= 100.0 {
            'o'
        } else {
            let f = t.charge / 100.0;
            if f >= 0.66 {
                '='
            } else if f >= 0.33 {
                '-'
            } else {
                '.'
            }
        });
    }
    let mut launchers = String::new();
    for l in &s.launchers {
        launchers.push(if l.sys.destroyed() {
            'x'
        } else if l.loaded.is_some() {
            'o'
        } else {
            '.'
        });
    }
    let mut rails = String::new();
    for r in &s.rails {
        rails.push(if r.sys.destroyed() {
            'x'
        } else if r.charge >= 100.0 {
            '\u{2261}'
        } else {
            '.'
        });
    }
    let mut shields = String::new();
    for sh in &s.shields {
        shields.push(if sh.sys.destroyed() {
            'x'
        } else if sh.state == ShieldState::Down {
            '_'
        } else {
            let f = sh.effective / 100.0;
            if f >= 0.75 {
                '\u{2588}'
            } else if f >= 0.4 {
                '\u{2592}'
            } else {
                '\u{2591}'
            }
        });
    }
    let mut drives = String::new();
    for dr in &s.drives {
        drives.push(if dr.sys.destroyed() {
            'x'
        } else {
            let f = ((dr.temp - 12.0) / 28.0).clamp(0.0, 1.0);
            if f >= 0.85 {
                '\u{2588}'
            } else if f >= 0.5 {
                '\u{2592}'
            } else {
                '\u{2591}'
            }
        });
    }
    let rows: Vec<(&str, String, &'static str)> = vec![
        ("Banks:", banks, GREEN),
        ("Tubes:", tubes, GREEN),
        ("Launchers:", launchers, GREEN),
        if s.rails.is_empty() {
            ("Shields:", shields.clone(), BGREEN)
        } else {
            ("Rails:", rails, GREEN)
        },
        if s.rails.is_empty() {
            ("Drives:", drives.clone(), GREY)
        } else {
            ("Shields:", shields.clone(), BGREEN)
        },
    ];
    let mut row = r0 + 2;
    for (label, value, color) in rows {
        put_str(grid, row, BOX_X + 3, label, GREEN);
        put_row(grid, row, BOX_X + 15, &value, color);
        row += 1;
    }
    if !s.rails.is_empty() {
        put_str(grid, row, BOX_X + 3, "Drives:", GREEN);
        put_row(grid, row, BOX_X + 15, &drives, GREY);
    }
}

/// Systems-status row values: destroyed mounts ('x') flash red.
fn put_row(grid: &mut [Vec<(char, &'static str)>], row: usize, col: usize, s: &str, color: &'static str) {
    for (i, ch) in s.chars().enumerate() {
        put(grid, row, col + i, ch, if ch == 'x' { RBLINK } else { color });
    }
}

/// Colorize a crew report line for the scroll region.
pub fn report_line(r: &begin_core::events::Report) -> Line {
    use begin_core::events::ReportKind;
    let color = match r.kind {
        ReportKind::Crew => CYAN,
        ReportKind::Ally => BGREEN,
        ReportKind::Alert => RED,
        ReportKind::Info => GREY,
        ReportKind::Blink => RBLINK,
    };
    if r.speaker.is_empty() {
        Line::new(format!("{color}{}{RESET}", r.text), r.text.chars().count())
    } else {
        Line::new(
            format!("{color}{}{RESET}{GREEN}: {}{RESET}", r.speaker, r.text),
            r.speaker.chars().count() + 2 + r.text.chars().count(),
        )
    }
}

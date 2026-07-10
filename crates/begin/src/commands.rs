//! The player command set (manual §VI + v2.0 notes + the port's additions:
//! `course^mark` 3D helm, railguns, fighters, planar lock).
//!
//! Commands only set flags/orders; the cycle resolves them. Status commands
//! do not end the turn (manual §IV STATUS).

use crate::ui::{self, Display, Line, CYAN, GREEN, GREY, RED, RESET, WHITE};
use begin_core::ai::Mission;
use begin_core::object::{Control, HelmMode, ObjId, ShieldState};
use begin_core::orders::{self, Mounts};
use begin_core::Game;

pub enum Outcome {
    /// Command consumed the turn: run a cycle.
    Advance,
    /// Status/display only: prompt again.
    Stay,
    Quit,
}

pub fn execute(g: &mut Game, me: ObjId, disp: &mut Display, input: &str) -> Outcome {
    let echo = format!("{GREY}> {input}{RESET}");
    disp.push(Line::new(echo, input.chars().count() + 2));
    let lower = input.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.is_empty() {
        return Outcome::Advance; // empty line lets time pass
    }
    let mut p = Parser { g, me, disp, w: &words, i: 0 };
    match p.run() {
        Ok(out) => out,
        Err(msg) => {
            p.disp.push(Line::new(format!("{RED}{msg}{RESET}"), msg.chars().count()));
            Outcome::Stay
        }
    }
}

struct Parser<'a> {
    g: &'a mut Game,
    me: ObjId,
    disp: &'a mut Display,
    w: &'a [&'a str],
    i: usize,
}

type R = Result<Outcome, String>;

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.w.get(self.i).copied()
    }
    fn next(&mut self) -> Option<&'a str> {
        let t = self.peek();
        if t.is_some() {
            self.i += 1;
        }
        t
    }
    fn skip_noise(&mut self) {
        while matches!(self.peek(), Some("to" | "the" | "on" | "at" | "with" | "a" | "an" | "and")) {
            self.i += 1;
        }
    }
    fn number(&mut self) -> Option<f64> {
        self.skip_noise();
        let t = self.peek()?;
        let v: f64 = t.parse().ok()?;
        self.i += 1;
        Some(v)
    }
    /// integer list like "1 2 3"; empty → All. "all" → All.
    fn mounts(&mut self) -> Mounts {
        if self.peek() == Some("all") {
            self.i += 1;
            return Mounts::All;
        }
        let mut list = Vec::new();
        while let Some(t) = self.peek() {
            if let Ok(n) = t.parse::<usize>() {
                list.push(n);
                self.i += 1;
            } else {
                break;
            }
        }
        if list.is_empty() {
            Mounts::All
        } else {
            Mounts::List(list)
        }
    }
    fn ship(&mut self) -> Result<ObjId, String> {
        self.skip_noise();
        let name = self.next().ok_or("Which ship?")?;
        self.g
            .find_by_name(name)
            .ok_or_else(|| format!("No ship named '{name}'."))
    }
    /// course token, possibly "320^22"
    fn course_mark(&mut self) -> Option<(f64, Option<f64>)> {
        self.skip_noise();
        let t = self.peek()?;
        if let Some((c, m)) = t.split_once('^') {
            let c: f64 = c.parse().ok()?;
            let m: f64 = m.parse().ok()?;
            self.i += 1;
            Some((c, Some(m)))
        } else {
            let c: f64 = t.parse().ok()?;
            self.i += 1;
            Some((c, None))
        }
    }
    fn say(&mut self, text: &str) {
        self.disp.push(Line::new(format!("{GREEN}{text}{RESET}"), text.chars().count()));
    }
    fn officer(&mut self, text: String) {
        let side = self.g.obj(self.me).nation;
        let officers = &self.g.data.nations[side].officers;
        let name = officers
            .get((self.g.cycle as usize + 1) % officers.len().max(1))
            .cloned()
            .unwrap_or_default();
        self.disp.push(Line::new(
            format!("{CYAN}{name}{RESET}{GREEN}: {text}{RESET}"),
            name.chars().count() + 2 + text.chars().count(),
        ));
    }

    fn run(&mut self) -> R {
        let cmd = self.next().unwrap();
        match cmd {
            "helm" | "h" => self.cmd_helm(),
            "pursue" => self.cmd_pursue(false),
            "elude" => self.cmd_pursue(true),
            "warp" | "speed" => {
                let w = self.number().ok_or("Warp factor (-1 to 20)?")?;
                orders::helm(self.g, self.me, None, None, Some(w));
                self.officer(format!("Warp factor {w}."));
                Ok(Outcome::Advance)
            }
            "chart" => {
                let mut lines = Vec::new();
                ui::chart_lines(self.g, self.me, &mut lines);
                for l in lines {
                    self.disp.push(l);
                }
                Ok(Outcome::Stay)
            }
            "report" => self.cmd_report(),
            "damage" => self.cmd_scan_ship(self.me),
            "scan" | "range" | "display" => self.cmd_scan(cmd),
            "fire" => self.cmd_fire(),
            "phaser" | "phasers" => self.cmd_fire_phasers(),
            "torp" | "torpedo" | "torpedoes" | "torpedos" => self.cmd_fire_torps(),
            "probe" | "probes" => self.cmd_fire_probes(),
            "rail" | "rails" | "railgun" | "railguns" => self.cmd_fire_rails(),
            "lock" => self.cmd_lock(),
            "turn" => self.cmd_turn(),
            "load" => self.cmd_load(),
            "unload" => self.cmd_unload(),
            "enable" | "disable" => self.cmd_enable(cmd == "enable"),
            "raise" | "lower" => self.cmd_shields(cmd == "raise"),
            "reenforce" | "reinforce" => {
                let n = self.number().ok_or("Which shield (1-6)?")? as usize;
                orders::reinforce_shield(self.g, self.me, Some(n.saturating_sub(1)));
                self.officer(format!("Reinforcing shield {n}."));
                Ok(Outcome::Advance)
            }
            "status" => self.cmd_status(),
            "banks" => self.status_banks(),
            "tubes" => self.status_tubes(),
            "launchers" => self.status_launchers(),
            "shields" => self.status_shields(),
            "fleet" => self.status_fleet(),
            "computer" | "library" => self.cmd_computer(),
            "help" | "?" => self.cmd_help(),
            "transport" | "beam" => self.cmd_transport(),
            "destruct" => self.cmd_destruct(),
            "abort" => {
                orders::abort_destruct(self.g, self.me);
                self.officer("Self destruct aborted.".into());
                Ok(Outcome::Advance)
            }
            "detonate" => self.cmd_detonate(),
            "repair" => self.cmd_repair(),
            "tractor" => self.cmd_tractor(),
            "board" => self.cmd_board(),
            "dock" => self.cmd_dock(),
            "undock" => {
                begin_core::systems::tractor::undock(self.g, self.me);
                self.officer("Undocking.".into());
                Ok(Outcome::Advance)
            }
            "cloak" => self.cmd_cloak(),
            "tell" | "order" => self.cmd_tell(),
            "launch" => self.cmd_launch_fighters(),
            "recover" => self.cmd_recover_fighters(),
            "flash" | "flashes" => {
                let on = self.peek() != Some("off");
                self.disp.flash = on;
                self.say(if on {
                    "Weapon flashes on."
                } else {
                    "Weapon flashes off."
                });
                Ok(Outcome::Stay)
            }
            "planarlock" => {
                let on = self.peek() == Some("on");
                self.g.tuning.planar_lock = on;
                self.say(if on { "Planar lock engaged (all ships confined to the plane)." } else { "Planar lock released." });
                Ok(Outcome::Stay)
            }
            "pass" | "wait" => Ok(Outcome::Advance),
            "quit" | "exit" => Ok(Outcome::Quit),
            _ => {
                // bare "<course> <warp>" helm shorthand
                self.i = 0;
                if let Some((c, m)) = self.course_mark() {
                    let w = self.number();
                    orders::helm(self.g, self.me, Some(c), m, w);
                    return Ok(Outcome::Advance);
                }
                Err(format!("Unknown command '{cmd}'.  Try 'help'."))
            }
        }
    }

    // ---- helm ----

    fn cmd_helm(&mut self) -> R {
        if self.peek() == Some("course") {
            self.i += 1;
        }
        let (c, m) = self.course_mark().ok_or("Course (0-360[^mark])?")?;
        if self.peek() == Some("warp") {
            self.i += 1;
        }
        let w = self.number();
        orders::helm(self.g, self.me, Some(c), m, w);
        let mtxt = m.map(|m| format!("^{m:.0}")).unwrap_or_default();
        self.officer(match w {
            Some(w) => format!("Coming to course {c:.0}{mtxt}, warp {w}."),
            None => format!("Coming to course {c:.0}{mtxt}."),
        });
        Ok(Outcome::Advance)
    }

    fn cmd_pursue(&mut self, elude: bool) -> R {
        let ship = self.ship()?;
        let w = self.number();
        orders::pursue(self.g, self.me, ship, w, elude);
        let name = self.g.obj(ship).name.clone();
        self.officer(format!("{} the {name}.", if elude { "Eluding" } else { "Pursuing" }));
        Ok(Outcome::Advance)
    }

    // ---- weapons ----

    fn cmd_fire(&mut self) -> R {
        let all = self.peek() == Some("all");
        if all {
            // "fire all phasers/banks/torpedoes/tubes/probes"
            self.i += 1;
        }
        match self.peek() {
            Some("phasers" | "phaser" | "banks" | "bank") => {
                self.i += 1;
                self.fire_phasers_args(all)
            }
            Some("torpedoes" | "torpedos" | "torpedo" | "torp" | "torps" | "tubes" | "tube") => {
                self.i += 1;
                self.cmd_fire_torps()
            }
            Some("probes" | "probe") => {
                self.i += 1;
                self.cmd_fire_probes()
            }
            Some("rails" | "railguns" | "rail") => {
                self.i += 1;
                self.cmd_fire_rails()
            }
            _ => Err("Fire what?  (phasers/banks, torpedoes/tubes, probes, rails)".into()),
        }
    }

    fn cmd_fire_phasers(&mut self) -> R {
        // top-level "phaser all 45" entry
        let mut all = false;
        while matches!(self.peek(), Some("phasers" | "phaser" | "banks" | "bank" | "all")) {
            if self.peek() == Some("all") {
                all = true;
            }
            self.i += 1;
        }
        self.fire_phasers_args(all)
    }

    /// Manual §VI: `[Fire] Phasers <list> [SPREAD] <spread>`,
    /// `Fire [ALL] Phasers [SPREAD] <spread>` — after ALL any number is the
    /// spread; in list form, numbers >= 10 (SPREAD_MIN, banks never reach
    /// index 10) are the spread.
    fn fire_phasers_args(&mut self, mut all: bool) -> R {
        const SPREAD_MIN: f64 = 10.0;
        while matches!(self.peek(), Some("phasers" | "phaser" | "banks" | "bank")) {
            self.i += 1;
        }
        if self.peek() == Some("all") {
            all = true;
            self.i += 1;
        }
        let mut list: Vec<usize> = Vec::new();
        let mut spread: Option<f64> = None;
        loop {
            match self.peek() {
                Some("spread" | "dispersion") => {
                    self.i += 1;
                    spread = self.number();
                }
                Some(t) => {
                    let Ok(v) = t.parse::<f64>() else { break };
                    self.i += 1;
                    if spread.is_none() && (all || v >= SPREAD_MIN) {
                        spread = Some(v);
                    } else {
                        list.push(v as usize);
                    }
                }
                None => break,
            }
        }
        let which = if all || list.is_empty() { Mounts::All } else { Mounts::List(list) };
        let n = orders::fire_phasers(self.g, self.me, &which, spread);
        if n == 0 {
            return Err("No phaser banks ready to fire.".into());
        }
        Ok(Outcome::Advance)
    }

    fn cmd_fire_torps(&mut self) -> R {
        while matches!(
            self.peek(),
            Some("all" | "torpedoes" | "torpedos" | "torpedo" | "torps" | "tubes" | "tube")
        ) {
            self.i += 1;
        }
        let which = self.mounts();
        let n = orders::fire_torpedoes(self.g, self.me, &which);
        if n == 0 {
            return Err("No loaded tubes ready to fire.".into());
        }
        Ok(Outcome::Advance)
    }

    fn cmd_fire_probes(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        let which = self.mounts();
        self.skip_noise();
        let (at, course) = match self.peek() {
            Some("course") => {
                self.i += 1;
                (None, Some(self.number().ok_or("Course?")?))
            }
            Some(_) => (Some(self.ship()?), None),
            None => {
                // fall back to launching at our current target-less course
                (None, Some(self.g.obj(self.me).course))
            }
        };
        let n = orders::fire_probes(self.g, self.me, &which, at, course);
        if n == 0 {
            return Err("No loaded launchers.".into());
        }
        Ok(Outcome::Advance)
    }

    fn cmd_fire_rails(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        let which = self.mounts();
        let n = orders::fire_rails(self.g, self.me, &which);
        if n == 0 {
            return Err("No charged railguns.".into());
        }
        Ok(Outcome::Advance)
    }

    fn cmd_lock(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        match self.next() {
            Some("banks" | "bank") => {
                let which = self.mounts();
                let ship = self.ship()?;
                if ship == self.me {
                    return Err("We can't lock weapons on ourselves.".into());
                }
                let hostile = self.g.obj(ship).nation != self.g.obj(self.me).nation;
                orders::lock_banks(self.g, self.me, &which, ship);
                let name = self.g.obj(ship).name.clone();
                if !hostile {
                    self.say(&format!("(Locking on the friendly {name}!)"));
                }
                self.officer(format!("Banks locked on the {name}."));
                Ok(Outcome::Advance)
            }
            Some("tubes" | "tube") => {
                let which = self.mounts();
                let ship = self.ship()?;
                if ship == self.me {
                    return Err("We can't lock weapons on ourselves.".into());
                }
                // `lock tubes on X [dispersion 20]` — fan the salvo (begin2's
                // third lock-tubes prompt); a bare trailing number counts too
                if matches!(self.peek(), Some("dispersion" | "spread")) {
                    self.i += 1;
                }
                let disp = self.number().unwrap_or(0.0);
                orders::lock_tubes(self.g, self.me, &which, ship, 0.0, disp);
                let name = self.g.obj(ship).name.clone();
                self.officer(if disp > 0.0 {
                    format!("Tubes locked on the {name}, dispersion {disp:.0}.")
                } else {
                    format!("Tubes locked on the {name}.")
                });
                Ok(Outcome::Advance)
            }
            Some("rails" | "rail") => {
                let which = self.mounts();
                let ship = self.ship()?;
                orders::lock_rails(self.g, self.me, &which, ship);
                self.officer("Railguns locked.".into());
                Ok(Outcome::Advance)
            }
            Some("probe" | "probes") => {
                let code = self.next().ok_or("Which probe (control code)?")?.to_string();
                let probe = begin_core::systems::probes::probe_by_code(self.g, self.me, &code)
                    .ok_or("No such probe.")?;
                let ship = self.ship()?;
                begin_core::systems::probes::lock_probe(self.g, probe, ship);
                self.officer(format!("Probe {code} locked."));
                Ok(Outcome::Advance)
            }
            _ => Err("Lock what?  (banks, tubes, probes)".into()),
        }
    }

    fn cmd_turn(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        match self.next() {
            Some("banks" | "bank") => {
                let which = self.mounts();
                let mark = self.number().ok_or("Mark angle?")?;
                orders::turn_banks(self.g, self.me, &which, mark);
                Ok(Outcome::Advance)
            }
            Some("tubes" | "tube") => {
                let which = self.mounts();
                let mark = self.number().ok_or("Mark angle?")?;
                if matches!(self.peek(), Some("dispersion" | "spread")) {
                    self.i += 1;
                }
                let disp = self.number().unwrap_or(0.0);
                orders::turn_tubes(self.g, self.me, &which, mark, disp);
                Ok(Outcome::Advance)
            }
            Some("probe" | "probes") => {
                let code = self.next().ok_or("Which probe (control code)?")?.to_string();
                let probe = begin_core::systems::probes::probe_by_code(self.g, self.me, &code)
                    .ok_or("No such probe.")?;
                let (c, m) = self.course_mark().ok_or("Course?")?;
                begin_core::systems::probes::turn_probe(self.g, probe, c, m.unwrap_or(0.0));
                Ok(Outcome::Advance)
            }
            _ => Err("Turn what?  (banks, tubes, probes)".into()),
        }
    }

    fn cmd_load(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        match self.next() {
            Some("tubes" | "tube" | "all") => {
                if self.peek() == Some("tubes") {
                    self.i += 1;
                }
                let which = self.mounts();
                if matches!(self.peek(), Some("prox" | "proximity")) {
                    self.i += 1;
                }
                let prox = self.number();
                orders::load_tubes(self.g, self.me, &which, prox);
                self.officer(match prox {
                    Some(p) => format!("Loading tubes, proximity {p:.0}."),
                    None => "Loading tubes.".into(),
                });
                Ok(Outcome::Advance)
            }
            Some("launchers" | "launcher" | "probes" | "probe") => {
                let which = self.mounts();
                if matches!(self.peek(), Some("prox" | "proximity")) {
                    self.i += 1;
                }
                let prox = self.number().unwrap_or(0.0);
                if matches!(self.peek(), Some("time" | "fuse")) {
                    self.i += 1;
                }
                let time = self.number().unwrap_or(0.0);
                let n = orders::load_launchers(self.g, self.me, &which, prox, time);
                if n == 0 {
                    return Err("No launchers could be loaded.".into());
                }
                // report the fuse settings actually applied (design-clamped)
                let detail = {
                    let s = self.g.obj(self.me).ship.as_ref().unwrap();
                    s.launchers
                        .iter()
                        .find_map(|l| l.loaded.as_ref())
                        .map(|p| {
                            let name = &self.g.data.probes[p.design].name;
                            format!("{n} {name} probe(s) loaded, prox {:.0}, time fuse {:.0}.", p.prox, p.time)
                        })
                        .unwrap_or_else(|| format!("{n} probe(s) loaded."))
                };
                self.officer(detail);
                Ok(Outcome::Advance)
            }
            _ => Err("Load what?  (tubes, launchers)".into()),
        }
    }

    fn cmd_unload(&mut self) -> R {
        if self.peek() == Some("all") {
            self.i += 1;
        }
        match self.next() {
            Some("tubes" | "tube" | "all") => {
                if self.peek() == Some("tubes") {
                    self.i += 1;
                }
                let which = self.mounts();
                orders::unload_tubes(self.g, self.me, &which);
                Ok(Outcome::Advance)
            }
            Some("launchers" | "launcher" | "probes") => {
                let which = self.mounts();
                orders::unload_launchers(self.g, self.me, &which);
                Ok(Outcome::Advance)
            }
            _ => Err("Unload what?".into()),
        }
    }

    fn cmd_enable(&mut self, enable: bool) -> R {
        match self.next() {
            Some("banks" | "bank") => {
                let which = self.mounts();
                orders::enable_banks(self.g, self.me, &which, enable);
                Ok(Outcome::Advance)
            }
            Some("tubes" | "tube") => {
                let which = self.mounts();
                // tubes enable/disable = allow loading
                let s = self.g.obj_mut(self.me).ship.as_mut().unwrap();
                for (k, t) in s.tubes.iter_mut().enumerate() {
                    if which.contains(k) {
                        t.loading_enabled = enable;
                    }
                }
                Ok(Outcome::Advance)
            }
            _ => Err("Enable/disable what?".into()),
        }
    }

    fn cmd_shields(&mut self, up: bool) -> R {
        if matches!(self.peek(), Some("shields" | "shield")) {
            self.i += 1;
        }
        let which = self.mounts();
        orders::set_shields(self.g, self.me, &which, up);
        self.officer(if up { "Shields up.".into() } else { "Shields down.".into() });
        Ok(Outcome::Advance)
    }

    // ---- status displays (no turn cost) ----

    fn cmd_status(&mut self) -> R {
        match self.next() {
            Some("damage") | None => self.cmd_scan_ship(self.me),
            Some("banks") => self.status_banks(),
            Some("tubes") => self.status_tubes(),
            Some("launchers") => self.status_launchers(),
            Some("shields") => self.status_shields(),
            Some("probes") => self.status_probes(),
            Some("fleet") => self.status_fleet(),
            Some(x) => Err(format!("No status display for '{x}'.")),
        }
    }

    fn cmd_scan(&mut self, cmd: &str) -> R {
        // "scan <ship>" or "scan/range/display <number>"
        self.skip_noise();
        if let Some(t) = self.peek() {
            if let Ok(r) = t.parse::<f64>() {
                self.disp.scan_range = r.clamp(1000.0, 500000.0);
                return Ok(Outcome::Stay);
            }
        } else {
            return Err(format!("{cmd} what?"));
        }
        let ship = self.ship()?;
        self.cmd_scan_ship(ship)
    }

    fn cmd_report(&mut self) -> R {
        let o = self.g.obj(self.me);
        let s = o.ship.as_ref().unwrap();
        let d = &self.g.data.ships[s.design];
        let navcom = match o.helm {
            HelmMode::Course => "Manual Helm".to_string(),
            HelmMode::Pursue => "Pursuit".to_string(),
            HelmMode::Elude => "Evasion".to_string(),
        };
        let lines = vec![
            format!("SHIP NAME:  {}", o.name),
            format!("CLASS:      {}", d.name),
            format!("SURVIVORS:  {}", s.survivors),
            format!("NAVCOM:     {navcom}"),
            format!("COURSE:     {:>6.0} desired   {:>6.0} current", o.desired_course, o.course),
            format!("MARK:       {:>6.0} desired   {:>6.0} current", o.desired_mark, o.mark),
            format!("WARP:       {:>6.1} desired   {:>6.1} current", o.desired_warp, o.warp),
        ];
        for l in lines {
            self.disp.push(Line::new(format!("{GREEN}{l}{RESET}"), l.chars().count()));
        }
        Ok(Outcome::Stay)
    }

    fn cmd_scan_ship(&mut self, id: ObjId) -> R {
        let side = self.g.obj(self.me).nation;
        if self.g.fog && id != self.me && !self.g.obj(id).contact(side).visible {
            return Err("No sensor contact.".into());
        }
        let o = self.g.obj(id);
        let s = o.ship.as_ref().unwrap();
        let d = &self.g.data.ships[s.design];
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("SHIP NAME:  {}", o.name));
        lines.push(format!("CLASS:      {}", d.name));
        lines.push(format!("SURVIVORS:  {}", s.survivors));
        lines.push(String::new());
        let pct = |v: &begin_core::object::Sys| -> String {
            if v.destroyed() {
                " XX ".into()
            } else {
                format!("{:>3}%", 100 - v.dmg)
            }
        };
        lines.push(format!(
            "REACTORS:   {}",
            s.reactors.iter().map(|r| pct(r)).collect::<Vec<_>>().join(" ")
        ));
        lines.push(format!(
            "BATTERIES:  {}",
            s.batteries
                .iter()
                .map(|b| if b.sys.destroyed() { " XX".into() } else { format!("{:>3.0}", b.charge) })
                .collect::<Vec<_>>()
                .join(" ")
        ));
        let bank_sym = |b: &begin_core::object::Bank| -> String {
            if b.sys.destroyed() {
                "XX".into()
            } else if b.charge >= d.banks_charge {
                "ch".into()
            } else {
                "..".into()
            }
        };
        lines.push(format!(
            "BANKS:      {}",
            s.banks.iter().map(bank_sym).collect::<Vec<_>>().join("  ")
        ));
        let tube_sym = |t: &begin_core::object::Tube| -> String {
            if t.sys.destroyed() {
                "XX".into()
            } else if t.loaded.is_some() && t.charge >= 100.0 {
                "ld".into()
            } else {
                "..".into()
            }
        };
        lines.push(format!(
            "TUBES:      {}",
            s.tubes.iter().map(tube_sym).collect::<Vec<_>>().join("  ")
        ));
        lines.push(format!(
            "LAUNCHERS:  {}",
            s.launchers
                .iter()
                .map(|l| if l.sys.destroyed() {
                    "XX".to_string()
                } else if l.loaded.is_some() {
                    "ld".to_string()
                } else {
                    "..".to_string()
                })
                .collect::<Vec<_>>()
                .join("  ")
        ));
        if !s.rails.is_empty() {
            lines.push(format!(
                "RAILS:      {}   ROUNDS: {}",
                s.rails
                    .iter()
                    .map(|r| if r.sys.destroyed() {
                        "XX".to_string()
                    } else if r.charge >= 100.0 {
                        "ch".to_string()
                    } else {
                        "..".to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("  "),
                s.rail_rounds_left
            ));
        }
        lines.push(format!(
            "SHIELDS:    {}",
            s.shields
                .iter()
                .map(|sh| if sh.sys.destroyed() {
                    " XX ".to_string()
                } else {
                    format!("{:>3.0}%", sh.effective)
                })
                .collect::<Vec<_>>()
                .join(" ")
        ));
        lines.push(format!(
            "WARP:       {}",
            s.drives
                .iter()
                .map(|dr| if dr.sys.destroyed() {
                    " XX ".to_string()
                } else {
                    format!("{:>3}%", 100 - dr.sys.dmg)
                })
                .collect::<Vec<_>>()
                .join(" ")
        ));
        let temps: Vec<String> = s
            .drives
            .iter()
            .map(|dr| {
                let arrow = if dr.temp_delta > 0.05 {
                    "\u{2191}"
                } else if dr.temp_delta < -0.05 {
                    "\u{2193}"
                } else {
                    ""
                };
                format!("{:.0}m{arrow}", dr.temp)
            })
            .collect();
        lines.push(format!("WARP TEMP:  {}   LIMIT: 40m", temps.join(" ")));
        lines.push(format!("WARP POWER:     {:>6.0}", o.warp_budget.max(0.0)));
        lines.push(format!("OTHER POWER:    {:>6.0}", begin_core::systems::power::gross_pool(self.g, id)));
        lines.push(format!(
            "RESIDUAL POWER: {:>+6.0}",
            o.residual
        ));
        if s.torps_left >= 0 {
            lines.push(format!("TORPS: {}   PROBES: {}", s.torps_left, s.probes_left));
        }
        for l in lines {
            self.disp.push(Line::new(format!("{GREEN}{l}{RESET}"), l.chars().count()));
        }
        Ok(Outcome::Stay)
    }

    fn status_banks(&mut self) -> R {
        let me = self.me;
        let (rows, full) = {
            let s = self.g.obj(me).ship.as_ref().unwrap();
            let full = self.g.data.ships[s.design].banks_charge;
            (s.banks.clone(), full)
        };
        self.header("BANK STATUS:");
        self.table_line("Bank  Level  Control  Mark  Status   Target");
        for (k, b) in rows.iter().enumerate() {
            let line = if b.sys.destroyed() {
                format!("{:>3}     ..  damaged   ...  .....    ..", k + 1)
            } else {
                let target =
                    b.lock.and_then(|t| self.g.get(t)).map(|t| t.name.clone()).unwrap_or("...".into());
                format!(
                    "{:>3}   {:>3.0}%  {:<7}  {:>4.0}  {:<8} {}",
                    k + 1,
                    b.charge / full * 100.0,
                    if b.lock.is_some() { "locked" } else { "manual" },
                    b.mark,
                    if !b.enabled {
                        "off"
                    } else if b.charge >= full {
                        "charged"
                    } else {
                        "drained"
                    },
                    target
                )
            };
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    fn status_tubes(&mut self) -> R {
        let me = self.me;
        let rows = self.g.obj(me).ship.as_ref().unwrap().tubes.clone();
        self.header("TUBES STATUS:");
        self.table_line("Tube  Level  Control  Mark  Content  Prox  Target");
        for (k, t) in rows.iter().enumerate() {
            let line = if t.sys.destroyed() {
                format!("{:>3}    ...  damaged   ...  .....     ..  ...", k + 1)
            } else {
                let content = t
                    .loaded
                    .as_ref()
                    .map(|l| self.g.data.torps[l.design].name.clone())
                    .unwrap_or("empty".into());
                let prox = t
                    .loaded
                    .as_ref()
                    .map(|l| format!("{:.0}", l.prox))
                    .unwrap_or("..".into());
                let target =
                    t.lock.and_then(|x| self.g.get(x)).map(|x| x.name.clone()).unwrap_or("...".into());
                format!(
                    "{:>3}   {:>3.0}%  {:<7}  {:>4.0}  {:<7}  {:>4}  {}",
                    k + 1,
                    t.charge,
                    if t.lock.is_some() { "locked" } else { "manual" },
                    t.mark,
                    content,
                    prox,
                    target
                )
            };
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    fn status_launchers(&mut self) -> R {
        let rows = self.g.obj(self.me).ship.as_ref().unwrap().launchers.clone();
        self.header("LAUNCHER STATUS:");
        self.table_line("Launcher  Status   Contents  Prox  Time  Code");
        for (k, l) in rows.iter().enumerate() {
            let line = if l.sys.destroyed() {
                format!("{:>5}     Damaged  ....       ...    ..  ....", k + 1)
            } else {
                match &l.loaded {
                    Some(p) => format!(
                        "{:>5}     Ready    {:<8}  {:>4.0}  {:>4.0}  {}",
                        k + 1,
                        self.g.data.probes[p.design].name,
                        p.prox,
                        p.time,
                        p.code
                    ),
                    None => format!("{:>5}     Ready    empty      ...    ..  ....", k + 1),
                }
            };
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    fn status_shields(&mut self) -> R {
        let me = self.me;
        let (rows, strength) = {
            let s = self.g.obj(me).ship.as_ref().unwrap();
            (s.shields.clone(), self.g.data.ships[s.design].shield_strength)
        };
        self.header("SHIELDS STATUS:");
        self.table_line("Shield  Field  Status   Functional  Effective   Regen.");
        for (k, sh) in rows.iter().enumerate() {
            let line = if sh.sys.destroyed() {
                format!("{:>4}    {:>4.0}.  damaged  ...  ...    ...  ...     ...", k + 1, sh.facing)
            } else {
                let func = strength * (100 - sh.sys.dmg) as f64 / 100.0;
                let eff = strength * sh.effective / 100.0;
                let state = match sh.state {
                    ShieldState::Up => "UP",
                    ShieldState::Down => "DOWN",
                    ShieldState::Reinforced => "REINF",
                };
                let regen = sh.strength / 100.0 * self.g.data.ships[self.g.obj(me).ship.as_ref().unwrap().design].shield_recharge;
                format!(
                    "{:>4}    {:>4.0}.  {:<6}  {:>4.0}eu {:>3}%  {:>4.0}eu {:>3.0}%  {:.2}%",
                    k + 1,
                    sh.facing,
                    state,
                    func,
                    100 - sh.sys.dmg,
                    eff,
                    sh.effective,
                    regen
                )
            };
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    fn status_probes(&mut self) -> R {
        let side = self.g.obj(self.me).nation;
        let mine: Vec<ObjId> = self
            .g
            .probe_ids()
            .into_iter()
            .filter(|&p| self.g.obj(p).owner == Some(self.me))
            .collect();
        if mine.is_empty() {
            return Err("We haven't any probes active.".into());
        }
        self.header("PROBE STATUS:");
        self.table_line("Code   Course  Bearing  Range  Prox  Time  Status  Range/Target");
        for p in mine {
            let o = self.g.obj(p);
            let st = o.probe.as_ref().unwrap();
            let (bearing, _) =
                begin_core::systems::helm::target_bearing_mark(self.g, self.me, p, side);
            let range = begin_core::systems::helm::dist(self.g, self.me, p);
            let tgt = o.pursue.and_then(|t| self.g.get(t));
            let tail = match tgt {
                Some(t) => {
                    let d = (t.pos - o.pos).len();
                    format!("{:>5.0} {}", d, t.name)
                }
                None => "....".into(),
            };
            let status = if st.arm > 0.0 { format!("arm {:.0}", st.arm) } else { "armed".into() };
            let line = format!(
                "{:<6} {:>5.0}  {:>6.0}  {:>5.0}  {:>4.0}  {:>4.0}  {:<6}  {}",
                st.code, o.course, bearing, range, st.prox, st.time, status, tail
            );
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    fn status_fleet(&mut self) -> R {
        self.header("ALLY      TARGET       MISSION");
        let side = self.g.obj(self.me).nation;
        for id in self.g.ship_ids() {
            if id == self.me || self.g.obj(id).nation != side {
                continue;
            }
            let o = self.g.obj(id);
            let s = o.ship.as_ref().unwrap();
            let target = s
                .brain
                .target
                .and_then(|t| self.g.get(t))
                .map(|t| {
                    if s.brain.target_ordered {
                        format!("({})", t.name)
                    } else {
                        t.name.clone()
                    }
                })
                .unwrap_or("..".into());
            let mission = match &s.brain.mission {
                None => {
                    if s.brain.stance == begin_core::ai::Stance::Retreat {
                        "Retreat".to_string()
                    } else {
                        "..".to_string()
                    }
                }
                Some(m) => mission_name(m),
            };
            let line = format!("{:<9} {:<12} {}", o.name, target, mission);
            self.table_line(&line);
        }
        Ok(Outcome::Stay)
    }

    // ---- library computer ----

    fn cmd_computer(&mut self) -> R {
        match self.next() {
            Some("ship") => {
                let nation = self.next().ok_or("Which nation?")?.to_string();
                let rest: Vec<&str> = self.w[self.i..].to_vec();
                if rest.is_empty() {
                    // list the nation's classes
                    let classes: Vec<String> = self
                        .g
                        .data
                        .classes_of(&nation)
                        .iter()
                        .map(|c| c.name.clone())
                        .collect();
                    if classes.is_empty() {
                        return Err(format!("Unknown nation '{nation}'."));
                    }
                    let l = classes.join(", ");
                    self.say(&l);
                    return Ok(Outcome::Stay);
                }
                let class = rest.join(" ");
                let d = self
                    .g
                    .data
                    .find_class(&nation, &class)
                    .cloned()
                    .ok_or_else(|| format!("No {nation} class '{class}'."))?;
                let lines = vec![
                    format!("{} {} ({})", d.nation, d.name, d.abbrev),
                    format!("CREW: {}   MASS: {}   MAX WARP: {}", d.crew, d.mass, d.max_warp),
                    format!(
                        "ACCEL: {}  DECEL: {}  TURN: {}  EFFICIENCY: {}",
                        d.w1accel, d.decel, d.w1turn, d.warp_efficiency
                    ),
                    format!(
                        "REACTORS: {}x{}  BATTERIES: {}x{}  DRIVES: {}x{}",
                        d.reactors, d.reactor_output, d.batteries, d.battery_capacity, d.drives, d.warp_power
                    ),
                    format!(
                        "BANKS: {} (chg {} rng {})  TUBES: {} ({})  LAUNCHERS: {} ({})",
                        d.banks,
                        d.banks_charge,
                        d.banks_range,
                        d.tubes,
                        d.torp.as_deref().unwrap_or("-"),
                        d.launchers,
                        d.probe.as_deref().unwrap_or("-")
                    ),
                    format!(
                        "SHIELDS: {}x{}eu (absorb {} regen {})",
                        d.shields, d.shield_strength, d.shield_absorption, d.shield_recharge
                    ),
                    format!(
                        "SCANNER: {}  REFLECT: {}  CLOAK: {}  TRACTOR: {}",
                        d.scanner_range,
                        d.scanner_reflect,
                        if d.can_cloak { "yes" } else { "no" },
                        d.tractor_strength
                    ),
                ];
                for l in lines {
                    self.disp.push(Line::new(format!("{GREEN}{l}{RESET}"), l.chars().count()));
                }
                Ok(Outcome::Stay)
            }
            Some("torpedo" | "torp") => {
                let name = self.next().map(str::to_string);
                match name {
                    None => {
                        let names: Vec<String> =
                            self.g.data.torps.iter().map(|t| t.name.clone()).collect();
                        self.say(&names.join(", "));
                        Ok(Outcome::Stay)
                    }
                    Some(n) => {
                        let t = self.g.data.torp(&n).cloned().ok_or("Unknown torpedo class.")?;
                        self.say(&format!("{} - {}", t.name, t.desc));
                        self.say(&format!(
                            "VELOCITY: warp {}   WARHEAD: {}   PROX: {}-{}",
                            t.velocity, t.damage, t.min_prox, t.max_prox
                        ));
                        self.say(&format!(
                            "ARM: {}  FUSE: {}  CHARGE TIME: {}  HOMING: {}",
                            t.arm_time,
                            t.max_time_fuse,
                            t.charge_time,
                            if t.homing { "yes" } else { "no" }
                        ));
                        Ok(Outcome::Stay)
                    }
                }
            }
            Some("probe") => {
                let name = self.next().map(str::to_string);
                match name {
                    None => {
                        let names: Vec<String> =
                            self.g.data.probes.iter().map(|p| p.name.clone()).collect();
                        self.say(&names.join(", "));
                        Ok(Outcome::Stay)
                    }
                    Some(n) => {
                        let p = self.g.data.probe(&n).cloned().ok_or("Unknown probe class.")?;
                        self.say(&format!("{} - {}", p.name, p.desc));
                        self.say(&format!(
                            "VELOCITY: warp {}   WARHEAD: {}   PROX: {}   SCAN: {}",
                            p.velocity, p.damage, p.max_prox, p.scan_range
                        ));
                        Ok(Outcome::Stay)
                    }
                }
            }
            _ => Err("Computer what?  (ship <nation> <class>, torpedo <t>, probe <p>)".into()),
        }
    }

    fn cmd_help(&mut self) -> R {
        let lines = [
            "Helm:     helm [course] C[^MARK] [warp] W | pursue/elude <ship> [W] | warp W",
            "Weapons:  fire [all] banks/phasers [list] [spread 10-45]   (fire all banks 45)",
            "          fire torpedoes <list> | fire probes at <ship> | fire rails",
            "          lock banks/tubes [list] on <ship> [dispersion D] | turn banks/tubes <list> M [D]",
            "          load tubes [prox P] | load launchers [prox P] [time T]",
            "Shields:  raise/lower shields | reenforce <#>",
            "Status:   report | damage | scan <ship|range> | banks|tubes|launchers|shields|probes|fleet",
            "Other:    transport N <ship> | tractor <ship>|off | board <ship> | dock <base> | cloak on",
            "          repair <system>|all | destruct | abort | detonate probe <code>|all | flash off",
            "Allies:   tell <ally|group N|all> attack <ship> | course C | escort <ship> R | standoff",
            "          ... torpedo/phaser/probe <ship> | dock <base> | tow <ship> <dest> | defend <ship>",
            "Library:  computer ship <nation> <class> | computer torpedo [t] | computer probe [p]",
        ];
        for l in lines {
            self.disp.push(Line::new(format!("{GREY}{l}{RESET}"), l.chars().count()));
        }
        Ok(Outcome::Stay)
    }

    // ---- misc systems ----

    fn cmd_transport(&mut self) -> R {
        let n = self.number().ok_or("Transport how many?")? as i32;
        let ship = self.ship()?;
        match begin_core::systems::boarding::transport(self.g, self.me, ship, n) {
            Ok(k) => {
                self.officer(format!("{k} crew transported."));
                Ok(Outcome::Advance)
            }
            Err(e) => Err(format!("Transporter room: {e}")),
        }
    }

    fn cmd_destruct(&mut self) -> R {
        if self.peek() == Some("probe") {
            self.i += 1;
            return self.cmd_detonate_probe();
        }
        orders::self_destruct(self.g, self.me).map_err(|e| e)?;
        self.officer("Self destruct in 5 cycles.".into());
        Ok(Outcome::Advance)
    }

    fn cmd_detonate(&mut self) -> R {
        while matches!(self.peek(), Some("probe" | "probes")) {
            self.i += 1;
        }
        self.cmd_detonate_probe()
    }

    /// `detonate probe <code>` / `detonate [all] probes` — remote-detonate.
    fn cmd_detonate_probe(&mut self) -> R {
        while matches!(self.peek(), Some("probe" | "probes")) {
            self.i += 1;
        }
        if matches!(self.peek(), Some("all") | None) {
            let n = begin_core::systems::probes::detonate_all(self.g, self.me);
            if n == 0 {
                return Err("We don't have any active probes.".into());
            }
            self.officer("Detonating all our active probes!".into());
            return Ok(Outcome::Advance);
        }
        let code = self.next().unwrap().to_string();
        let probe = begin_core::systems::probes::probe_by_code(self.g, self.me, &code)
            .ok_or("No probe responds to that code.")?;
        begin_core::systems::probes::detonate_probe(self.g, probe);
        self.officer(format!("Probe \"{code}\" detonated."));
        Ok(Outcome::Advance)
    }

    fn cmd_repair(&mut self) -> R {
        use begin_core::object::RepairClass::*;
        let what = self.next().unwrap_or("all");
        let class = match what {
            "all" | "none" => None,
            "launchers" | "launcher" => Some(Launchers),
            "banks" | "bank" | "phasers" | "rails" => Some(Banks),
            "tubes" | "tube" => Some(Tubes),
            "drives" | "drive" | "warp" => Some(Drives),
            "shields" | "shield" => Some(Shields),
            "transporter" | "transporters" => Some(Transporter),
            "cloak" => Some(Cloak),
            "reactors" | "reactor" => Some(Reactors),
            "batteries" | "battery" => Some(Batteries),
            "scanner" | "sensors" => Some(Scanner),
            "impulse" => Some(Impulse),
            "tractor" => Some(Tractor),
            x => return Err(format!("Unknown system '{x}'.")),
        };
        self.g.obj_mut(self.me).ship.as_mut().unwrap().repair_priority = class;
        self.officer(match class {
            Some(_) => format!("Damage control concentrating on {what}."),
            None => "Damage control working normally.".into(),
        });
        Ok(Outcome::Advance)
    }

    fn cmd_tractor(&mut self) -> R {
        self.skip_noise();
        if matches!(self.peek(), Some("off" | "release")) {
            let s = self.g.obj_mut(self.me).ship.as_mut().unwrap();
            s.tractor_engaged = false;
            s.tractor_target = None;
            self.officer("Tractor beam released.".into());
            return Ok(Outcome::Advance);
        }
        let ship = self.ship()?;
        begin_core::systems::tractor::engage_tractor(self.g, self.me, ship).map_err(|e| e)?;
        let name = self.g.obj(ship).name.clone();
        self.officer(format!("Tractor beam locked on the {name}."));
        Ok(Outcome::Advance)
    }

    fn cmd_board(&mut self) -> R {
        let ship = self.ship()?;
        match begin_core::systems::boarding::transport(self.g, self.me, ship, i32::MAX) {
            Ok(n) => {
                self.officer(format!("Boarding party of {n} away."));
                Ok(Outcome::Advance)
            }
            Err(e) => Err(format!("We can't board: {e}")),
        }
    }

    fn cmd_dock(&mut self) -> R {
        self.skip_noise();
        if self.peek() == Some("off") {
            begin_core::systems::tractor::undock(self.g, self.me);
            return Ok(Outcome::Advance);
        }
        let base = self.ship()?;
        begin_core::systems::tractor::dock(self.g, self.me, base).map_err(|e| e)?;
        Ok(Outcome::Advance)
    }

    fn cmd_cloak(&mut self) -> R {
        let on = self.peek() != Some("off");
        orders::set_cloak(self.g, self.me, on).map_err(|e| e)?;
        self.officer(if on { "Cloaking device engaged.".into() } else { "Decloaking.".into() });
        Ok(Outcome::Advance)
    }

    // ---- fighters (near-future battlestars) ----

    fn cmd_launch_fighters(&mut self) -> R {
        if matches!(self.peek(), Some("fighters" | "fighter" | "vipers")) {
            self.i += 1;
        }
        let n = self.number().map(|v| v as usize).unwrap_or(usize::MAX);
        let launched = crate::fighters::launch_fighters(self.g, self.me, n)?;
        self.officer(format!("{launched} fighters away."));
        Ok(Outcome::Advance)
    }

    fn cmd_recover_fighters(&mut self) -> R {
        if matches!(self.peek(), Some("fighters" | "fighter" | "vipers")) {
            self.i += 1;
            let n = crate::fighters::recall_fighters(self.g, self.me);
            self.officer(format!("Recalling {n} fighters."));
            return Ok(Outcome::Advance);
        }
        // "recover <ship>": dock a ship to us (base recovery)
        let ship = self.ship()?;
        begin_core::systems::tractor::dock(self.g, ship, self.me).map_err(|e| e)?;
        Ok(Outcome::Advance)
    }

    // ---- ally orders ----

    fn cmd_tell(&mut self) -> R {
        self.skip_noise();
        let side = self.g.obj(self.me).nation;
        // addressee: ship name, "group N", or "all"
        let addr = self.next().ok_or("Tell whom?")?;
        let allies: Vec<ObjId> = if addr == "all" {
            self.g
                .ship_ids()
                .into_iter()
                .filter(|&i| i != self.me && self.g.obj(i).nation == side && self.g.obj(i).control == Control::Ai)
                .collect()
        } else if addr == "group" {
            let n = self.number().ok_or("Which group?")? as u32;
            self.g
                .ship_ids()
                .into_iter()
                .filter(|&i| {
                    i != self.me
                        && self.g.obj(i).nation == side
                        && self.g.obj(i).ship.as_ref().unwrap().group == Some(n)
                })
                .collect()
        } else {
            let ship = self
                .g
                .find_by_name(addr)
                .ok_or_else(|| format!("No ship named '{addr}'."))?;
            if self.g.obj(ship).nation != side {
                return Err("They are not answering our hails.".into());
            }
            if ship == self.me {
                return Err("Talking to ourselves again?".into());
            }
            vec![ship]
        };
        if allies.is_empty() {
            return Err("No one is listening.".into());
        }
        let order = self.parse_order()?;
        for ally in allies {
            match &order {
                OrderKind::Mission(m) => {
                    begin_core::ai::missions::receive_order(self.g, ally, m.clone());
                }
                OrderKind::Cancel => {
                    begin_core::ai::missions::cancel_order(self.g, ally);
                    let captain = self.g.obj(ally).ship.as_ref().unwrap().captain.clone();
                    let n2 = captain.clone();
                    self.g.say(
                        Some(side),
                        &n2,
                        "Disengaging.".into(),
                        begin_core::events::ReportKind::Ally,
                    );
                }
                OrderKind::OpenFire => {
                    self.g.obj_mut(ally).ship.as_mut().unwrap().brain.hold_fire = false;
                }
                OrderKind::Report => {
                    let o = self.g.obj(ally);
                    let s = o.ship.as_ref().unwrap();
                    let text = format!(
                        "Report: crew {}, warp {:.1}, course {:.0}, shields {:.0}%.",
                        s.survivors,
                        o.warp,
                        o.course,
                        s.shields.iter().map(|x| x.effective).sum::<f64>()
                            / s.shields.len().max(1) as f64
                    );
                    let captain = s.captain.clone();
                    self.g.say(Some(side), &captain, text, begin_core::events::ReportKind::Ally);
                }
                OrderKind::Join(n) => {
                    self.g.obj_mut(ally).ship.as_mut().unwrap().group = Some(*n);
                }
                OrderKind::Leave => {
                    self.g.obj_mut(ally).ship.as_mut().unwrap().group = None;
                }
                OrderKind::Retreat => {
                    let b = &mut self.g.obj_mut(ally).ship.as_mut().unwrap().brain;
                    b.stance = begin_core::ai::Stance::Retreat;
                    b.mission = None;
                }
            }
        }
        Ok(Outcome::Advance)
    }

    fn parse_order(&mut self) -> Result<OrderKind, String> {
        self.skip_noise();
        let verb = self.next().ok_or("Order them to do what?")?;
        Ok(match verb {
            "attack" | "target" | "engage" => {
                OrderKind::Mission(Mission::Attack { ship: self.ship()? })
            }
            "disengage" | "cancel" => OrderKind::Cancel,
            "escort" => {
                let ship = self.ship()?;
                let range = self.number().unwrap_or(2000.0);
                OrderKind::Mission(Mission::Escort { ship, range })
            }
            "course" => OrderKind::Mission(Mission::Course {
                course: self.number().ok_or("Course?")?,
            }),
            "hold" => {
                self.next(); // "fire"
                OrderKind::Mission(Mission::HoldFire)
            }
            "open" => {
                self.next(); // "fire"
                OrderKind::OpenFire
            }
            "retreat" => OrderKind::Retreat,
            "withdraw" | "standoff" => OrderKind::Mission(Mission::Standoff),
            "report" => OrderKind::Report,
            "probe" => OrderKind::Mission(Mission::Probe { ship: self.ship()? }),
            "phaser" | "phasers" => OrderKind::Mission(Mission::Phaser { ship: self.ship()? }),
            "torpedo" | "torpedoes" | "torp" => {
                OrderKind::Mission(Mission::Torpedo { ship: self.ship()? })
            }
            "transport" | "beam" => {
                let n = self.number().ok_or("How many crew?")? as i32;
                OrderKind::Mission(Mission::Transport { count: n, ship: self.ship()? })
            }
            "dock" => OrderKind::Mission(Mission::Dock { base: self.ship()? }),
            "undock" => OrderKind::Mission(Mission::Undock),
            "tow" => {
                let ship = self.ship()?;
                let dest = self.ship()?;
                OrderKind::Mission(Mission::Tow { ship, dest })
            }
            "release" => OrderKind::Mission(Mission::Release),
            "tractor" => OrderKind::Mission(Mission::Tractor { ship: self.ship()? }),
            "approach" => {
                let ship = self.ship()?;
                let range = self.number().unwrap_or(1000.0);
                OrderKind::Mission(Mission::Approach { ship, range })
            }
            "stop" => OrderKind::Mission(Mission::Stop),
            "join" => OrderKind::Join(self.number().ok_or("Which group?")? as u32),
            "leave" => OrderKind::Leave,
            "defend" => OrderKind::Mission(Mission::Defend { ship: self.ship()? }),
            "recover" => OrderKind::Mission(Mission::Recover { ship: self.ship()? }),
            "eject" => OrderKind::Mission(Mission::Eject { ship: self.ship()? }),
            x => return Err(format!("They don't understand '{x}'.")),
        })
    }

    fn header(&mut self, s: &str) {
        self.disp.push(Line::new(format!("{WHITE}{s}{RESET}"), s.chars().count()));
    }
    fn table_line(&mut self, s: &str) {
        self.disp.push(Line::new(format!("{GREEN}{s}{RESET}"), s.chars().count()));
    }
}

enum OrderKind {
    Mission(Mission),
    Cancel,
    OpenFire,
    Report,
    Retreat,
    Join(u32),
    Leave,
}

fn mission_name(m: &Mission) -> String {
    match m {
        Mission::Escort { .. } => "Escort".into(),
        Mission::Attack { .. } => "Attack".into(),
        Mission::Course { course } => format!("Course {course:.0}"),
        Mission::Phaser { .. } => "Phaser".into(),
        Mission::Torpedo { .. } => "Torpedo".into(),
        Mission::Probe { .. } => "Probe".into(),
        Mission::Standoff => "Standoff".into(),
        Mission::Transport { .. } => "Transport".into(),
        Mission::Dock { .. } => "Dock".into(),
        Mission::Undock => "Undock".into(),
        Mission::Tow { .. } => "Tow".into(),
        Mission::Release => "Release".into(),
        Mission::Recover { .. } => "Recover".into(),
        Mission::Eject { .. } => "Eject".into(),
        Mission::Approach { .. } => "Approach".into(),
        Mission::Tractor { .. } => "Tractor".into(),
        Mission::Stop => "Stop".into(),
        Mission::Defend { .. } => "Defend".into(),
        Mission::HoldFire => "Hold fire".into(),
    }
}

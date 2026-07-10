//! Crew chatter and combat reports. The core emits fully-formatted lines
//! tagged with an audience and a color class; front-ends only render them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportKind {
    /// Player's own crew / command echo (screenshots: cyan speaker)
    Crew,
    /// Ally captain chatter (green)
    Ally,
    /// Enemy-related or damage drama (red)
    Alert,
    /// Neutral notices, e.g. "** kdat **" sensor contacts
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Which nation-side sees this line; None = everyone.
    pub side: Option<usize>,
    /// Speaker name shown before the colon ("Sulu", "Kromm"); empty = bare line.
    pub speaker: String,
    pub text: String,
    pub kind: ReportKind,
    pub cycle: u64,
}

impl Report {
    /// Should a viewer on `side` see this line?
    pub fn visible_to(&self, side: usize) -> bool {
        self.side.is_none() || self.side == Some(side)
    }
}

/// A transient visual effect the front-end may overlay on its scope for a
/// fraction of a second (phaser fire, detonations). Purely cosmetic.
#[derive(Debug, Clone, Copy)]
pub enum Flash {
    /// Hitscan beam (phaser or railgun) from a ship along its firing axis.
    Beam { from: crate::math::Vec3, to: crate::math::Vec3 },
    /// A detonation with its splash radius.
    Blast { pos: crate::math::Vec3, radius: f64 },
}

/// Collects the lines produced during a cycle. Front-ends (or the multiplayer
/// host) take the whole batch after each cycle and filter per viewer side.
#[derive(Debug, Default)]
pub struct Reporter {
    pub lines: Vec<Report>,
    pub flashes: Vec<Flash>,
}

impl Reporter {
    pub fn say(
        &mut self,
        side: Option<usize>,
        speaker: &str,
        text: String,
        kind: ReportKind,
        cycle: u64,
    ) {
        self.lines.push(Report { side, speaker: speaker.to_string(), text, kind, cycle });
    }
    pub fn take(&mut self) -> Vec<Report> {
        std::mem::take(&mut self.lines)
    }
    pub fn take_flashes(&mut self) -> Vec<Flash> {
        std::mem::take(&mut self.flashes)
    }
}

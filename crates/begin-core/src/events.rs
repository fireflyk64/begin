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

/// Collects the lines produced during a cycle. Front-ends (or the multiplayer
/// host) take the whole batch after each cycle and filter per viewer side.
#[derive(Debug, Default)]
pub struct Reporter {
    pub lines: Vec<Report>,
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
}

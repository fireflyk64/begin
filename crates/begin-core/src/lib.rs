//! begin-core — the simulation engine of BEGIN: A Tactical Starship
//! Simulation, ported from the reverse-engineered begin2.exe
//! (see AI_AND_COMBAT.md). No I/O here; front-ends drive `Game::run_cycle`
//! and render `Reporter` lines + game state.

pub mod ai;
pub mod constants;
pub mod cycle;
pub mod data;
pub mod env;
pub mod events;
pub mod game;
pub mod math;
pub mod object;
pub mod orders;
pub mod scenario;
pub mod systems;

pub use constants::Tuning;
pub use data::GameData;
pub use game::Game;
pub use object::{Control, ObjId};

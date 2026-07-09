//! Peer-to-peer multiplayer over lobbylink (~/dev/lobbylink).
//!
//! True P2P: no game server. The hosting player's process owns the
//! simulation (begin2's control type 2 = remote human, `sub_20F6A`);
//! every other player is a dumb terminal — they see the ANSI frames the
//! host renders for them and type command lines back. Turn-based like the
//! original: the cycle runs once every seated player has entered a
//! turn-ending command (status displays answer immediately).
//!
//! Wire protocol (reliable channel, JSON):
//!   client → host: {"t":"line","text":"fire all phasers"}
//!   host → client: {"t":"frame","data":"<ansi screen>"}
//!                  {"t":"info","text":"..."}   (lobby chatter)
//!                  {"t":"over","text":"evaluation"}

use crate::commands::{self, Outcome};
use crate::ui::{self, Display, GREEN, GREY, RED, RESET, WHITE};
use begin_core::object::{Control, ObjId};
use begin_core::scenario::{spawn_fleets, Scenario};
use begin_core::{Game, GameData, Tuning};
use p2p_lobby_client::{ConnectOptions, CreateOptions, Event, MessageKind, P2PGame, PlayerId};
use std::collections::HashMap;
use std::io::Write;

pub const DEFAULT_SERVER: &str = "https://pqrstuvw.xyz/lobbylink";
const DEFAULT_ORIGIN: &str = "https://pqrstuvw.xyz";

/// The prod signaling server requires an allowlisted Origin header;
/// local/dev servers run with --allow-no-origin and want none.
fn origin_for(server: &str) -> Option<String> {
    if server == DEFAULT_SERVER {
        Some(DEFAULT_ORIGIN.into())
    } else {
        None
    }
}

fn msg(t: &str, key: &str, val: &str) -> bytes::Bytes {
    bytes::Bytes::from(serde_json::json!({"t": t, key: val}).to_string())
}

pub struct HostConfig {
    pub server: String,
    pub code: String,
    pub players: u16,
    pub versus: bool,
    pub scenario: Scenario,
    pub tuning: Tuning,
    pub seed: u64,
    pub name: String,
}

struct Seat {
    ship: Option<ObjId>,
    disp: Display,
    submitted: bool,
    connected: bool,
    name: String,
    queue: std::collections::VecDeque<String>,
}

/// Host: create the room, wait for everyone, run the simulation.
pub async fn run_host(cfg: HostConfig) -> Result<(), Box<dyn std::error::Error>> {
    println!("{GREY}Creating room on {} for {} players...{RESET}", cfg.server, cfg.players);
    let mut lobby = P2PGame::connect(ConnectOptions {
        server: cfg.server.clone(),
        code: cfg.code.clone(),
        create: Some(CreateOptions {
            wait_until_full: true,
            allow_reconnect: true,
            ..CreateOptions::new(cfg.players)
        }),
        origin: origin_for(&cfg.server),
        ..Default::default()
    })
    .await?;
    println!(
        "{WHITE}Room code: {}{RESET}  (players join with: begin join {}{})",
        lobby.code(),
        lobby.code(),
        if cfg.server == DEFAULT_SERVER { String::new() } else { format!(" --server {}", cfg.server) }
    );

    // wait for the room to fill
    let mut connected: Vec<PlayerId> = lobby
        .players()
        .iter()
        .filter(|p| p.occupied && p.id != lobby.self_id())
        .map(|p| p.id)
        .collect();
    while (connected.len() as u16) + 1 < cfg.players {
        match lobby.next_event().await {
            Some(Event::PlayerJoined { player_id }) => {
                println!("{GREEN}Player {player_id} has joined.{RESET}");
                if !connected.contains(&player_id) {
                    connected.push(player_id);
                }
            }
            Some(Event::PeerState { .. }) | Some(Event::CandidatePair { .. }) => {}
            Some(Event::Started) => break,
            Some(e) => println!("{GREY}{e:?}{RESET}"),
            None => return Err("lobby closed while waiting".into()),
        }
    }
    println!("{GREEN}All hands aboard. Spawning fleets...{RESET}");

    // build the game
    let data = GameData::load();
    let mut game = Game::new(data, cfg.tuning.clone(), cfg.seed);
    begin_core::env::setup(&mut game, cfg.scenario.epoch_jd, cfg.scenario.spawn_body.as_deref(), cfg.seed);
    let fleets = spawn_fleets(&mut game, &cfg.scenario).map_err(|e| e.to_string())?;

    // seat assignment: host takes the ally flagship; remote players take
    // the enemy flagship first in --versus, then remaining ally ships.
    let mut seats: HashMap<PlayerId, Seat> = HashMap::new();
    let my_ship = fleets.flagship.or(fleets.ally_ids.first().copied());
    let mut pool: Vec<ObjId> = Vec::new();
    if cfg.versus {
        pool.extend(fleets.enemy_ids.iter().copied());
    }
    pool.extend(fleets.ally_ids.iter().copied().filter(|&s| Some(s) != my_ship));
    if cfg.versus {
        // interleave leftover: enemy first already in pool order
    }
    connected.sort_unstable();
    for (k, pid) in connected.iter().enumerate() {
        let ship = pool.get(k).copied();
        let pname = format!("Player {pid}");
        if let Some(s) = ship {
            game.obj_mut(s).control = Control::Remote(pname.clone());
        }
        seats.insert(
            *pid,
            Seat {
                ship,
                disp: Display::default(),
                submitted: false,
                connected: true,
                name: pname,
                queue: std::collections::VecDeque::new(),
            },
        );
    }

    // host seat
    let mut my_disp = Display::default();
    let host_side = my_ship.map(|s| game.obj(s).nation).unwrap_or(0);

    // initial charts + frames
    for (_pid, seat) in seats.iter_mut() {
        push_chart(&game, seat.ship, &mut seat.disp);
    }
    push_chart(&game, my_ship, &mut my_disp);
    for (pid, seat) in seats.iter() {
        let frame = render_for(&game, seat);
        let _ = lobby.send_reliable(*pid, msg("frame", "data", &frame)).await;
    }
    draw_local(&game, my_ship, &my_disp, &cfg.name);

    // local stdin task
    let (tx, mut local_lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut my_submitted = false;
    let mut my_queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut quit = false;

    'outer: loop {
        tokio::select! {
            line = local_lines.recv() => {
                let Some(line) = line else { break };
                my_queue.push_back(line);
            }
            ev = lobby.next_event() => {
                let Some(ev) = ev else { break };
                match ev {
                    Event::Message { from, kind: MessageKind::Reliable, data } => {
                        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) else { continue };
                        if v["t"] == "line" {
                            if let Some(seat) = seats.get_mut(&from) {
                                seat.queue.push_back(v["text"].as_str().unwrap_or("").to_string());
                            }
                        }
                    }
                    Event::PlayerLeft { player_id, .. } => {
                        if let Some(seat) = seats.get_mut(&player_id) {
                            seat.connected = false;
                            if let Some(s) = seat.ship.filter(|&s| game.get(s).is_some()) {
                                game.obj_mut(s).control = Control::Ai;
                            }
                            my_disp.push(ui::Line::new(
                                format!("{RED}{} has left; their ship reverts to its AI captain.{RESET}", seat.name), 50));
                            draw_local(&game, my_ship, &my_disp, &cfg.name);
                        }
                    }
                    Event::PlayerRejoined { player_id, .. } => {
                        if let Some(seat) = seats.get_mut(&player_id) {
                            seat.connected = true;
                            if let Some(s) = seat.ship.filter(|&s| game.get(s).is_some()) {
                                game.obj_mut(s).control = Control::Remote(seat.name.clone());
                            }
                            let f = render_for(&game, seat);
                            let _ = lobby.send_reliable(player_id, msg("frame", "data", &f)).await;
                        }
                    }
                    _ => {}
                }
            }
        }

        // pump queued lines into submissions; run cycles as turns complete
        loop {
            // host lines
            while !my_submitted {
                let Some(line) = my_queue.pop_front() else { break };
                match my_ship.filter(|&s| game.get(s).is_some()) {
                    Some(s) => match commands::execute(&mut game, s, &mut my_disp, line.trim()) {
                        Outcome::Quit => {
                            quit = true;
                            break 'outer;
                        }
                        Outcome::Stay => draw_local(&game, my_ship, &my_disp, &cfg.name),
                        Outcome::Advance => {
                            my_submitted = true;
                            draw_local(&game, my_ship, &my_disp, &cfg.name);
                        }
                    },
                    None => {
                        quit = true;
                        break 'outer;
                    }
                }
            }
            // remote lines
            let pids: Vec<PlayerId> = seats.keys().copied().collect();
            for pid in pids {
                let seat = seats.get_mut(&pid).unwrap();
                while !seat.submitted {
                    let Some(line) = seat.queue.pop_front() else { break };
                    match seat.ship.filter(|&s| game.get(s).is_some()) {
                        Some(s) => match commands::execute(&mut game, s, &mut seat.disp, line.trim()) {
                            Outcome::Quit => {
                                if let Some(o) = game.get_mut(s) {
                                    o.control = Control::Ai;
                                }
                                seat.ship = None;
                            }
                            Outcome::Stay => {
                                let f = render_for(&game, seat);
                                let _ = lobby.send_reliable(pid, msg("frame", "data", &f)).await;
                            }
                            Outcome::Advance => {
                                seat.submitted = true;
                                let f = render_for(&game, seat);
                                let _ = lobby.send_reliable(pid, msg("frame", "data", &f)).await;
                            }
                        },
                        None => break,
                    }
                }
            }

            // every live seat locked in?
            let everyone = my_submitted
                && seats
                    .values()
                    .all(|s| s.submitted || !s.connected || s.ship.is_none());
            if !everyone {
                break;
            }

            game.run_cycle();
            crate::fighters::absorb_docked_fighters(&mut game);
            let reports = game.reporter.take();
            for (_pid, seat) in seats.iter_mut() {
                seat.submitted = false;
                if let Some(s) = seat.ship.filter(|&s| game.get(s).is_some()) {
                    let side = game.obj(s).nation;
                    for r in &reports {
                        if r.visible_to(side) {
                            seat.disp.push(ui::report_line(r));
                        }
                    }
                    push_chart(&game, Some(s), &mut seat.disp);
                } else if seat.ship.is_some() {
                    seat.disp.push(ui::Line::new(format!("{RED}Your ship has been destroyed.{RESET}"), 30));
                    seat.ship = None;
                }
            }
            my_submitted = false;
            for r in &reports {
                if r.visible_to(host_side) {
                    my_disp.push(ui::report_line(r));
                }
            }
            let my_alive = my_ship.map(|s| game.get(s).is_some()).unwrap_or(false);
            if my_alive {
                push_chart(&game, my_ship, &mut my_disp);
            }
            for (pid, seat) in seats.iter() {
                if seat.connected {
                    let f = render_for(&game, seat);
                    let _ = lobby.send_reliable(*pid, msg("frame", "data", &f)).await;
                }
            }
            draw_local(&game, my_ship.filter(|&s| game.get(s).is_some()), &my_disp, &cfg.name);
            if game.over.is_some() || !my_alive {
                break 'outer;
            }
        }
    }
    let _ = quit;

    // evaluations, per side
    for (pid, seat) in seats.iter() {
        if !seat.connected {
            continue;
        }
        let side = seat
            .ship
            .and_then(|s| game.get(s).map(|o| o.nation))
            .unwrap_or(host_side);
        let (_t, text) = game.evaluation(side);
        let _ = lobby.send_reliable(*pid, msg("over", "text", &text)).await;
    }
    let (_t, text) = game.evaluation(host_side);
    println!("\r\n{WHITE}{}{RESET}\r\n{GREEN}{text}{RESET}", game.data.nations[host_side].command);
    // let the reliable channel flush the evaluations before we leave
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    lobby.close().await?;
    Ok(())
}

fn push_chart(game: &Game, ship: Option<ObjId>, disp: &mut Display) {
    if let Some(s) = ship.filter(|&s| game.get(s).is_some()) {
        disp.push(ui::Line::plain(""));
        let mut chart = Vec::new();
        ui::chart_lines(game, s, &mut chart);
        for l in chart {
            disp.push(l);
        }
    }
}

fn render_for(game: &Game, seat: &Seat) -> String {
    ui::render(game, seat.ship.filter(|&s| game.get(s).is_some()), &seat.disp, &seat.name)
}

fn draw_local(game: &Game, ship: Option<ObjId>, disp: &Display, name: &str) {
    let frame = ui::render(game, ship, disp, name);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(frame.as_bytes());
    let _ = out.flush();
}

/// Client: a dumb terminal. Print what the host sends; send what we type.
pub async fn run_client(server: String, code: String) -> Result<(), Box<dyn std::error::Error>> {
    println!("{GREY}Joining room {code} on {server}...{RESET}");
    let origin = origin_for(&server);
    let mut lobby = P2PGame::connect(ConnectOptions {
        server,
        code,
        origin,
        ..Default::default()
    })
    .await?;
    println!("{GREEN}Connected as player {}. Waiting for the host to start...{RESET}", lobby.self_id());

    // the host is the lowest-numbered occupied seat that isn't us
    let host: PlayerId = lobby
        .players()
        .iter()
        .filter(|p| p.occupied && p.id != lobby.self_id())
        .map(|p| p.id)
        .min()
        .unwrap_or(0);

    let (tx, mut local_lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            line = local_lines.recv() => {
                let Some(line) = line else { break };
                let payload = serde_json::json!({"t": "line", "text": line}).to_string();
                if lobby.send_reliable(host, bytes::Bytes::from(payload)).await.is_err() {
                    println!("{RED}Lost the host.{RESET}");
                    break;
                }
                if line.trim().eq_ignore_ascii_case("quit") {
                    break;
                }
            }
            ev = lobby.next_event() => {
                let Some(ev) = ev else { break };
                match ev {
                    Event::Message { kind: MessageKind::Reliable, data, .. } => {
                        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) else { continue };
                        match v["t"].as_str() {
                            Some("frame") => {
                                let mut out = std::io::stdout().lock();
                                let _ = out.write_all(v["data"].as_str().unwrap_or("").as_bytes());
                                let _ = out.flush();
                            }
                            Some("info") => println!("{GREY}{}{RESET}", v["text"].as_str().unwrap_or("")),
                            Some("over") => {
                                println!("\r\n{GREEN}{}{RESET}", v["text"].as_str().unwrap_or(""));
                                lobby.close().await?;
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                    Event::PlayerLeft { player_id, .. } if player_id == host => {
                        // catch a final evaluation that may still be in flight
                        let grace = tokio::time::sleep(std::time::Duration::from_millis(700));
                        tokio::pin!(grace);
                        loop {
                            tokio::select! {
                                _ = &mut grace => break,
                                ev = lobby.next_event() => {
                                    let Some(Event::Message { kind: MessageKind::Reliable, data, .. }) = ev else { continue };
                                    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) else { continue };
                                    if v["t"] == "over" {
                                        println!("\r\n{GREEN}{}{RESET}", v["text"].as_str().unwrap_or(""));
                                        break;
                                    }
                                }
                            }
                        }
                        println!("{RED}The host has left. Game over.{RESET}");
                        break;
                    }
                    Event::SignalingClosed { code, .. } if code != "connection-lost" => break,
                    _ => {}
                }
            }
        }
    }
    lobby.close().await?;
    Ok(())
}

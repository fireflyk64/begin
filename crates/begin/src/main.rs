//! BEGIN — A Tactical Starship Simulation.
//! Rust port of begin2.exe console mode (see AI_AND_COMBAT.md).

mod commands;
mod fighters;
mod net;
mod ui;

use begin_core::scenario::{spawn_fleets, FleetEntry, Scenario, SideConfig};
use begin_core::{Game, GameData, Tuning};
use commands::Outcome;
use std::io::{BufRead, Write};
use ui::{Display, BGREEN, CYAN, GREEN, GREY, RED, RESET, WHITE};

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    // multiplayer subcommands: `begin host ...` / `begin join <code> ...`
    let mode = if args.len() > 1 && (args[1] == "host" || args[1] == "join") {
        args.remove(1)
    } else {
        String::new()
    };
    let mut server = net::DEFAULT_SERVER.to_string();
    let mut code = String::new();
    let mut players: u16 = 2;
    let mut versus = true;
    let mut tuning = Tuning::default();
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1);
    let mut quick = false;
    let mut epoch = 0.0f64;
    let mut spawn_body: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                i += 1;
                seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(seed);
            }
            "--quick" => quick = true,
            "--planar-lock" => tuning.planar_lock = true,
            "--begin1" => tuning = Tuning::begin1(),
            "--date" => {
                i += 1;
                epoch = args
                    .get(i)
                    .map(|s| begin_core::env::parse_epoch(s))
                    .unwrap_or(0.0);
            }
            "--near" => {
                i += 1;
                spawn_body = args.get(i).cloned();
            }
            "--server" => {
                i += 1;
                server = args.get(i).cloned().unwrap_or(server);
            }
            "--code" => {
                i += 1;
                code = args.get(i).cloned().unwrap_or_default();
            }
            "--players" => {
                i += 1;
                players = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(2);
            }
            "--coop" => versus = false,
            "--versus" => versus = true,
            x if mode == "join" && code.is_empty() && !x.starts_with('-') => {
                code = x.to_string();
            }
            x => {
                eprintln!("unknown option {x}");
                eprintln!("usage: begin [host|join <code>] [--quick] [--seed N] [--planar-lock] [--begin1]");
                eprintln!("             [--date YYYY-MM-DD] [--near Body:low|high|rings]");
                eprintln!("             [--server URL] [--code C] [--players N] [--coop|--versus]");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    println!("{WHITE}BEGIN - A Tactical Starship Simulation{RESET}");
    println!("{GREY}Rust port of Begin 2.00 (c) 1984-1991 Clockwork Software{RESET}");
    println!();

    if mode == "join" {
        if code.is_empty() {
            eprintln!("usage: begin join <code> [--server URL]");
            std::process::exit(2);
        }
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        if let Err(e) = rt.block_on(net::run_client(server, code)) {
            eprintln!("{RED}{e}{RESET}");
            std::process::exit(1);
        }
        return;
    }

    let data = GameData::load();

    if mode == "host" {
        // configure the scenario on the host terminal, then open the room.
        // NB: the stdin lock must be released before run_host spawns its
        // own stdin reader thread.
        let (scenario, name) = if quick {
            let mut sc = Scenario::duel();
            sc.epoch_jd = epoch;
            sc.spawn_body = spawn_body.clone();
            sc.seed = seed;
            (sc, "Admiral".to_string())
        } else {
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            match setup_scenario(&data, seed, epoch, spawn_body, &mut lines) {
                Some(x) => x,
                None => return,
            }
        };
        let cfg = net::HostConfig {
            server,
            code,
            players,
            versus,
            scenario,
            tuning,
            seed,
            name,
        };
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        if let Err(e) = rt.block_on(net::run_host(cfg)) {
            eprintln!("{RED}{e}{RESET}");
            std::process::exit(1);
        }
        return;
    }

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let (scenario, name) = if quick {
        let mut sc = Scenario::duel();
        sc.epoch_jd = epoch;
        sc.spawn_body = spawn_body.clone();
        sc.seed = seed;
        (sc, "Admiral".to_string())
    } else {
        match setup_scenario(&data, seed, epoch, spawn_body, &mut lines) {
            Some(x) => x,
            None => return,
        }
    };
    let mut game = Game::new(data, tuning, seed);
    setup_environment(&mut game, &scenario);
    let fleets = match spawn_fleets(&mut game, &scenario) {
        Ok(f) => f,
        Err(e) => {
            println!("{RED}{e}{RESET}");
            return;
        }
    };
    let me = fleets.flagship.expect("ally flagship");

    let side = game.obj(me).nation;
    let mut disp = Display::default();
    disp.push_plain("");
    let mut chart = Vec::new();
    ui::chart_lines(&game, me, &mut chart);
    for l in chart {
        disp.push(l);
    }

    let stdout = std::io::stdout();
    loop {
        // draw
        {
            let mut out = stdout.lock();
            let frame = ui::render(&game, game.get(me).map(|_| me), &disp, &name);
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
        }
        // read
        let Some(Ok(line)) = lines.next() else { break };
        let outcome = if game.get(me).is_some() && game.over.is_none() {
            commands::execute(&mut game, me, &mut disp, line.trim())
        } else {
            Outcome::Quit
        };
        match outcome {
            Outcome::Quit => break,
            Outcome::Stay => continue,
            Outcome::Advance => {
                game.run_cycle();
                fighters::absorb_docked_fighters(&mut game);
                for r in game.reporter.take() {
                    if r.visible_to(side) {
                        disp.push(ui::report_line(&r));
                    }
                }
                if game.get(me).is_some() {
                    let mut chart = Vec::new();
                    ui::chart_lines(&game, me, &mut chart);
                    disp.push_plain("");
                    for l in chart {
                        disp.push(l);
                    }
                } else {
                    disp.push(ui::Line::new(
                        format!("{RED}Your ship has been destroyed.{RESET}"),
                        30,
                    ));
                    break;
                }
                if game.over.is_some() {
                    break;
                }
            }
        }
    }

    // endgame evaluation (§13)
    println!("\r\n");
    let (_tier, text) = game.evaluation(side);
    let nation = &game.data.nations[side];
    println!("{WHITE}{}{RESET}", nation.command);
    println!("{GREEN}{text}{RESET}");
}

fn setup_environment(g: &mut Game, sc: &Scenario) {
    begin_core::env::setup(g, sc.epoch_jd, sc.spawn_body.as_deref(), sc.seed);
}

fn prompt(msg: &str, lines: &mut std::io::Lines<std::io::StdinLock>) -> Option<String> {
    print!("{CYAN}{msg}{RESET} ");
    std::io::stdout().flush().ok();
    lines.next()?.ok().map(|s| s.trim().to_string())
}

fn setup_scenario(
    data: &GameData,
    seed: u64,
    epoch: f64,
    spawn_body: Option<String>,
    lines: &mut std::io::Lines<std::io::StdinLock>,
) -> Option<(Scenario, String)> {
    let name = loop {
        let n = prompt("What is your name, commander?", lines)?;
        if !n.is_empty() {
            break n;
        }
    };
    let nations: Vec<String> = data.nations.iter().map(|n| n.adjective.clone()).collect();
    let nation_list = nations.join(", ");
    let ally_nation = loop {
        let n = prompt(&format!("Your nation? ({nation_list})"), lines)?;
        if let Some(nn) = data.nation(&n) {
            break nn.adjective.clone();
        }
        println!("{RED}Unknown nation.{RESET}");
    };
    let enemy_nation = loop {
        let n = prompt(&format!("Enemy nation? ({nation_list})"), lines)?;
        if let Some(nn) = data.nation(&n) {
            if nn.adjective != ally_nation {
                break nn.adjective.clone();
            }
            println!("{RED}A civil war?  Pick someone else.{RESET}");
        } else {
            println!("{RED}Unknown nation.{RESET}");
        }
    };

    println!();
    println!(
        "{WHITE}FLEET SETUP{RESET}  (up to {} ships per side)",
        begin_core::scenario::MAX_FLEET
    );
    println!("{GREY}Commands: ally <n> <class> [flagship] | enemy <n> <class> | flagship <class>{RESET}");
    println!("{GREY}          begin | random (spread out, sensors dark) | quit{RESET}");
    {
        let ally_classes: Vec<String> =
            data.classes_of(&ally_nation).iter().map(|c| c.name.clone()).collect();
        let enemy_classes: Vec<String> =
            data.classes_of(&enemy_nation).iter().map(|c| c.name.clone()).collect();
        println!("{BGREEN}{ally_nation}{RESET}: {}", ally_classes.join(", "));
        println!("{RED}{enemy_nation}{RESET}: {}", enemy_classes.join(", "));
    }

    let mut ally = SideConfig { nation: ally_nation.clone(), ships: Vec::new(), flagship: None };
    let mut enemy = SideConfig { nation: enemy_nation.clone(), ships: Vec::new(), flagship: None };
    let random_placement;

    loop {
        let line = prompt(&format!("{name} (setup)>"), lines)?;
        let lower = line.to_ascii_lowercase();
        let w: Vec<&str> = lower.split_whitespace().collect();
        if w.is_empty() {
            continue;
        }
        match w[0] {
            "begin" | "random" => {
                if ally.ships.is_empty() || enemy.ships.is_empty() {
                    println!("{RED}Both fleets need at least one ship.{RESET}");
                    continue;
                }
                random_placement = w[0] == "random";
                break;
            }
            "quit" | "exit" => return None,
            "ally" | "enemy" | "config" | "configure" => {
                let mut idx = 1;
                let is_ally = if w[0] == "config" || w[0] == "configure" {
                    let side = w.get(1).copied().unwrap_or("ally");
                    idx = 2;
                    side == "ally"
                } else {
                    w[0] == "ally"
                };
                let side_cfg = if is_ally { &mut ally } else { &mut enemy };
                let nation = if is_ally { &ally_nation } else { &enemy_nation };
                let mut k = idx;
                let mut added = false;
                while k < w.len() {
                    let Ok(n) = w[k].parse::<usize>() else { break };
                    k += 1;
                    let mut class_words = Vec::new();
                    let mut flag = false;
                    while k < w.len() && w[k].parse::<usize>().is_err() {
                        if w[k] == "flagship" {
                            flag = true;
                        } else {
                            class_words.push(w[k]);
                        }
                        k += 1;
                    }
                    let class = class_words.join(" ");
                    match data.find_class(nation, &class) {
                        Some(d) => {
                            let cname = d.name.clone();
                            side_cfg
                                .ships
                                .push(FleetEntry { class: cname.clone(), count: n.clamp(1, 9) });
                            if flag && is_ally {
                                side_cfg.flagship = Some(cname.clone());
                            }
                            println!(
                                "{GREEN}{} {} x{}{}{RESET}",
                                nation,
                                cname,
                                n.clamp(1, 9),
                                if flag { " (flagship)" } else { "" }
                            );
                            added = true;
                        }
                        None => println!("{RED}No {nation} class '{class}'.{RESET}"),
                    }
                }
                if !added {
                    println!("{GREY}e.g.  ally 2 heavy cruisers flagship{RESET}");
                }
            }
            "flagship" => {
                let class = w[1..].join(" ");
                match data.find_class(&ally_nation, &class) {
                    Some(d) => {
                        ally.flagship = Some(d.name.clone());
                        println!("{GREEN}Flagship: {}{RESET}", d.name);
                    }
                    None => println!("{RED}No such class.{RESET}"),
                }
            }
            _ => println!("{RED}Setup commands: ally/enemy/flagship/begin/random/quit{RESET}"),
        }
    }

    if ally.flagship.is_none() {
        ally.flagship = ally.ships.first().map(|e| e.class.clone());
    }
    let sc = Scenario { ally, enemy, stations: Vec::new(), random_placement, seed, epoch_jd: epoch, spawn_body };
    Some((sc, name))
}

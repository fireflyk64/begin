use begin_core::object::Control;
use begin_core::scenario::{spawn_fleets, Scenario};
use begin_core::{Game, GameData, Tuning};

#[test]
#[ignore]
fn watch_ai_duel() {
    let mut g = Game::new(GameData::load(), Tuning::default(), 7);
    let mut sc = Scenario::duel();
    sc.ally.flagship = None;
    let fleets = spawn_fleets(&mut g, &sc).unwrap();
    for id in fleets.ally_ids.iter().chain(fleets.enemy_ids.iter()) {
        g.obj_mut(*id).control = Control::Ai;
    }
    let mut cycles = 0;
    while g.over.is_none() && cycles < 2500 {
        g.run_cycle();
        for r in g.reporter.take() {
            println!("[{:4}] {}{}{}", r.cycle,
                if r.speaker.is_empty() { String::new() } else { format!("{}: ", r.speaker) },
                r.text,
                if r.side.is_some() { format!("  (side {})", r.side.unwrap()) } else { String::new() });
        }
        if cycles % 200 == 0 {
            for id in g.ship_ids() {
                let o = g.obj(id);
                let s = o.ship.as_ref().unwrap();
                println!("  c{cycles} {} w{:.1} crs{:.0} crew {} torps {} shields {:.0}",
                    o.name, o.warp, o.course, s.survivors, s.torps_left,
                    s.shields.iter().map(|x| x.effective).sum::<f64>());
            }
        }
        cycles += 1;
    }
    println!("=== over: {:?} after {cycles} cycles", g.over);
}

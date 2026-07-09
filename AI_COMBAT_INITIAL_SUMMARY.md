# Begin2 AI & Combat — Initial Research Summary

Status snapshot of the reverse-engineering effort on `begin2_annotated.asm`
(IDA disassembly of begin2.exe), in preparation for the Rust rewrite.
Date: 2026-07-04. All line numbers below refer to `begin2_annotated.asm` in
this directory (note: the file contains high-bit bytes — use `grep -a`).

**Coverage: roughly 90% of the AI and combat engine is decoded**, including
the AI behavior tree Daniel tuned, with the actual tuning constants extracted
(including the two patched per `begin2/notes.txt`: `phaserDamMult = 45.0`,
line 99290, and `ooDestructDamage = 9.0` → tweaked value 14.0, line 99239).

The full implementation-grade spec (`AI_AND_COMBAT.md`) is the next
deliverable and has **not** been written yet. No Rust code exists yet.

---

## Game architecture

From `XXSimulate` (line 5374). One cycle =

1. Per-ship control dispatch — control type at node+9Ch: 1 = local human
   (`getInput`, 62887), 2 = remote human (`sub_20F6A`) — begin2 was already
   multiplayer-ready, 3 = AI (`sub_20D55`, 62774), 4 = none.
2. The `detonation` pipeline (26446) which runs the entire simulation step:
   - `sub_C3B9` (19191) clear locks on dying objects
   - `sub_C4F7` (19337) power generation, drives, warp temperature, life
     support, cloak/tractor energy; calls `sub_C883` (19700) shields
     power/regen
   - `sub_CA97` (19930) phaser resolution → `phaserDamage` (23256)
   - `sub_CB5F` (20043) torpedo launch → `sub_E9B5` (23463)
   - `sub_CC27` (20154) probe launch; `sub_CCE2` (20260) warp accel/decel;
     `sub_CEB8` (20451) turning + homing guidance + velocity vectors
   - ~20 movement sub-steps interleaved with prox-fuse checks:
     `sub_D0DC` (20667) integrate, `sub_D2D2` (20900) prox fuses,
     `splash_starter` (21127); fast path `sub_1032E` (26336+) when no
     ordnance in flight
   - `sub_D130` (20709) arm/time fuses; `sub_D72F` (21416) splash resolution
   - `sub_C010` (18795) boarding combat; `sub_100B2` (26135 block) tractor
     pull; `sub_102A7` tractor auto-release; `sub_DE7C` (22201) battery
     recharge + residual power + tow position lock
3. End-condition check `sub_1044E`, endgame rating `sub_104BB` (7 tiers).

Movement: positions are X/Y doubles at node+34h/+3Ch; bearings compass-style
degrees (0 = +Y, computed via atan(dx/dy), rad→deg 57.2957, quadrant fix
+180); each object carries a per-substep velocity vector (node+44h/+4Ch) =
(sin, cos)(course) × current_warp × 5.0, i.e. ≈ warp × 100 units per cycle.

Object lists: all objects 378C9, ships 378C1, torps 378B9, probes 378B1.

## Ship physics (`sub_CCE2` accel, `sub_CEB8` turn)

Exactly the manual's rules, now with formulas:

- Instant helm response when current and desired warp both ≤ 1; ships also
  turn instantly at warp ≤ 1.
- Acceleration = `w1accel / current_warp`; deceleration flat; both capped by
  remaining difference.
- Turn rate = `w1turn / current_warp`, always the shorter way.
- Max attainable warp = `available_power / warp_power_use`, capped at design
  max; warp-1 fallback if the impulse engine lives (`sub_EFB2`, 24112).
- Helm modes at node+6Eh: 0 = course, 1 = pursue (re-aims at target every
  cycle), 2 = elude (aims directly away every cycle).

## Power model (`sub_C4F7`, `sub_EED2`, `sub_F101`)

Reactors (× health) + battery charge feed a per-cycle pool (node+5Ch).
Drains in order: warp (desired_warp × warp_power_use), shields
(shield_energy each, ×4 when reinforced — states 7 up / 8 down / 9
reinforced), phaser charging, cloak_energy, tractor_energy. If shields can't
be paid they collapse and a brain flag is set (this is what gates the AI's
shield micromanagement). Leftover recharges batteries (`sub_F101`, 24276);
the remainder is the RESIDUAL POWER display (node+64h); average battery fill
fraction is stored in brain+94h. Life support costs crew/10 per cycle;
3 consecutive failures kill the crew.

Warp temperature (`sub_F259`, 24428): floor 12, `+= (warp_ratio −
drive_health × warp_efficiency) × 4` per cycle, drive destroyed at 40.
This is the manual's "WARP TEMP: 12m ↓15m LIMIT: 40m".

## Weapons

- **Phasers** (`phaserDamage`, 23256): cone hitscan, damage =
  `charge × (45.0 / spread) × sqrt(1 − dist/range) × 0.5`, applied to
  **everything** in the cone including friends. Charging pulls
  `banks_charge / banks_rate` per cycle from the pool (`sub_B1C1`, 16697).
  Bank can bear if |angle off| < 22.5°.
- **Torpedoes**: intercept lead via arcsine solution (`sub_B926`, 17715):
  `mark = rel_bearing + asin(sin(angle_off) × target_warp / torp_velocity)`;
  speed variance; arm time; prox fuse checked each sub-step vs **all** ships
  (friendly fire is real); time-fuse expiry.
- **Probes**: slow homing weapons, re-aim every cycle, remotely
  detonatable; prox triggers only on enemies (or deliberate targets).
- **Boarding** (`sub_C010`, 18795): round-by-round crew vs boarders using
  nation Boarding Combat Level; capture when crew < 6 — captured ship
  switches nation and gets a fresh AI brain (`sub_1D4BD`).
- Weapon slot structs: banks at body+116h stride 2Ah, tubes at body+268h
  stride 3Ah, launchers at body+43Ch stride 1Ch; fire/lock/load executors
  `sub_BAD0`..`sub_BFBF` (17954–18770) set flags that the pipeline resolves.

## The AI (the part to preserve)

Priority-ordered behavior tree, dispatcher `sub_20D55` (62774), context
setup `sub_1E08A` (57274). Per-ship "brain" (0xA0 bytes at body+22h) with
personality rolled at spawn (`sub_1D4BD`, 55844):
`clamp01(nation_value ± uniform(deviation))` for aggression / bravery /
loyalty / fanaticism, plus a persistent left/right weave preference.

- **Reflexes** (`sub_20C86` chain, 62632): slow to warp 1 when drive temp
  > 38 with asymmetric drive damage (`sub_1E59D`); flee imminent detonations
  (`sub_1E6AC`); snap-fire phasers at the nearest enemy pointed at you,
  computing *how many banks* are needed to break the facing shield and firing
  exactly that many (`sub_1E78A`); phaser point-defense vs incoming probes,
  including rotating a bank to bear and decelerating to keep a chasing probe
  in envelope (`sub_1E8B8`/`sub_20697`); reinforce weakest shield when power
  allows (`sub_1E9BB`), drop reinforcement when it doesn't (`sub_1EA71`).
- **Targeting** (`sub_1E30D` + scorer `sub_207F9`, 62096): score = target
  strength × distance-band multiplier (×3.0 at <2000 falling to ×1.1 at
  18–20k, ×0.5 beyond; table at ds:0xA65C, dumped near line 101584) × 2.0 if
  inside own phaser range × 1.25 current-target stickiness; halved if other
  probes already chase it; halved if it's already outnumbered relative to
  (1−bravery)×5; ×0.1 for stale sensor contacts. Player-ordered targets
  obeyed with probability ~loyalty×2 (refusals draw from 5 insult lines,
  pointer table at ds:0xA648).
- **Fire control**: torpedo volley sizing (`sub_2047A`) tracks a per-target
  "incoming torpedo pressure" counter (brain+8Ah, capped 90) — fresh targets
  get full spreads, saturated ones occasional singles; torps aimed 20°
  off-axis at actively-dodging targets (`sub_1EC17`); prox auto-set to
  `target_max_warp × 50` capped by design max (`sub_1F09F`); friendly-fire
  arc check (`sub_20398`) blocks shots with an ally in the corridor and the
  AI sidesteps ±90° to clear the line (`sub_201C7`).
- **Maneuver** (`sub_2001A`, `sub_20120`, `sub_201C7`, `sub_2029B`,
  `sub_2051F`, `sub_205DF`): zigzag weave with amplitude growing with threat
  proximity and shrinking with aggression; standoff = `min_torp_range +
  (1−aggression) × (max−min)`; combat speed = max warp × drive health ×
  warp_efficiency with temperature panic (enter 0.85, exit 0.4 hysteresis);
  slows down to complete sharp turns; steering updates at most every 2
  cycles (brain+1Ch cooldown).
- **Morale** (`sub_1EFC8`, `sub_1DB78`, `sub_1DB12`, `sub_1DA17`,
  `sub_1DC5E`): retreat when damage ratio > bravery×0.75, return below
  bravery×0.5; retreat toward a friendly base if any (8 flavored lines at
  ds:0xA628); fanatics (> 0.9) set self-destruct (5-cycle countdown) and ram
  — this is why Romulan War Eagles behave the way they do; captains remotely
  detonate their own probes when the target outruns them but is still inside
  blast radius (`sub_20AF8`); Romulans cloak when idle (`sub_20BF2`);
  Orion-style boarding when crew advantage suffices (`sub_1E018`/`sub_20C3C`).
- **All 22 ally-order missions** (`sub_1F3B8`, 59564, jump table at end):
  1 escort, 2 attack, 3 course, 4 phaser, 5 torpedo, 6 probe, 7 standoff
  (20–30k, aggression-scaled), 8 transport, 9 dock, 10 undock, 11 tow (speed
  = tractor_strength/mass), 12 release, 14 recover, 15 eject, 16 approach,
  17 tractor, 18 stop, 19 join group, 20 leave group, 22 defend (attacks
  whoever is closest to the defendee); loyalty-gated refusal via
  `sub_200BD`.

**Sensor model** (`sub_E673`, 23114): objects out of scanner range keep
last-known position/course/warp (node+0A6h..+0CCh); the whole AI honors the
stale data; the chart shows ghosts.

Ship strength estimate (`sub_106F7`, 26909): `1 + charged_banks ×
banks_range/1000 + loaded_tubes × 6`, cached in brain (+4Ah phaser part,
+52h torp part, +5Ah total, +62h at-spawn max).

## Remaining un-decoded (~10%)

| Item | Where |
|---|---|
| Splash damage falloff detail | `sub_D72F`, line 21416 (721 instr, partially read) |
| Hull/system damage allocation, crew casualties | `dealDamageToHull`, line 25192 (730 instr) |
| Shield face application detail in splash | `splashDamage` tail, ~22600–23250 |
| Repair mechanics | location not yet confirmed (what looked like repair was boarding); repair-rate constants exist in stats headers |
| Torpedo spawn details | `sub_E9B5`, 23463 |
| Transporter/beaming | `sub_16897` (41286), `sub_16A70` (41544) |
| `.sce` scenario format fine points | loader `sub_21A7C`, 64518 |
| A handful of `[VERIFY]` constants | e.g. `dbl_332AE` (fast-path step), `flt_332AA` (tractor range scale) |

All have known line numbers; finishing them is cheap (~40–60k tokens).

## Next planned steps (awaiting approval)

1. Optionally finish the two damage functions above.
2. Write `AI_AND_COMBAT.md` — the precise, implementation-complete spec with
   asm line references throughout, so a fresh model/session can implement the
   Rust version (or continue the research) from compact context.
3. Pause before any Rust implementation, per Daniel's instruction.

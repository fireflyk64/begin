# BEGIN2 — AI & Combat Specification and Rust Port Plan

**Purpose.** This document is the implementation-grade specification for the
Rust rewrite of *BEGIN: A Tactical Starship Simulation* (begin2.exe console
mode). It captures ~100% of the reverse-engineered simulation, combat, and AI
behavior from `begin2_annotated.asm`, precisely enough to implement the game
without re-reading the assembly — while every section carries **asm
references** (function name + line number in `begin2_annotated.asm`) so a
fresh session can go deeper wherever needed during implementation.

**How to use the references.** `begin2_annotated.asm` is an IDA dump of
begin2.exe. It contains high-bit bytes, so always search with `grep -a`.
`begin2_annotated.asm.inventory.tsv` lists every function with size and one
xref. Line numbers below are exact where given as plain numbers, approximate
where prefixed `≈`. Companion references in this repo:

| File | Content |
|---|---|
| `begin2/stats200.h` | All ship/torpedo/probe/nation data as C initializers (v2.00 stats) |
| `begin2/stats165.h` | Same for v1.65 |
| `begin2/shipeditor/shipdesign.h`, `torpdesign.h`, `probedesign.h` | Exact binary field layouts of the design structs |
| `manual-pdf.pdf` | v1.65 manual + v2.0 release notes: full player command set, display layouts |
| `begin2/notes.txt` | Daniel's binary patch notes (AI hook offsets, tuned constants) |
| `AI_COMBAT_INITIAL_SUMMARY.md` | Shorter narrative summary of this research |

**Daniel's tuning (from `begin2/notes.txt`)** — the shipped binary was
patched during his years of tweaking; the Rust port should expose these as
config and default to his values:
- `ooDestructDamage` (asm 99239): default 9.0, **tuned to 14.0** ("seems to
  work nicely") — divisor→scaler for ship self-destruct yield (see §10.4).
- "For begin2 to act like begin: modify float 45.0 → 16.3125, then multiply
  all splash damage weapons to have .3625 as much damage" — the 45.0 is
  `phaserDamMult` (99290); the splash scaler is `SplashDamMult` = 5.0
  (99276). These two knobs select "begin1 feel" vs "begin2 feel".

---

## Part I — Original Requirements (the product being built)

Rust rewrite of begin2 console mode with these properties (Daniel's spec):

1. **Simplicity first** — fixed-size arrays for ships/objects (begin's engine
   used linked lists of a fixed pool; we use arenas/fixed `Vec` capacity),
   cycle-based simulation. "What made begin so successful was the simplicity
   and understandability of it all."
2. **Flexible enough for near-future combat**: battlestar/carrier hulls with
   fighters, kinetic railguns (0.01–0.1% c — near-instant cone hitscan at
   game ranges), and unguided kinetic rounds, alongside the classic
   phaser/torpedo/probe set.
3. **3D movement**: course + **mark** angle relative to the reference plane
   (helm syntax like `320^22`). Ships spawn coplanar. Pursue/elude must track
   targets out of plane. A server-side **planar lock** mode confines all
   objects to the plane (in case out-of-plane makes missiles/railguns too
   easy to dodge).
4. **Environment**: `anise` + `de440.bsp` (+ JPL data for moons and minor
   planets like Ceres) when kernels are configured; built-in VSOP87 / JPL
   "Approximate Positions of the Planets" Keplerian fallback otherwise.
   Self-contained module, small code footprint; startup config carries the
   epoch date. Space stations attach to named planets/moons at low or
   geosynchronous orbit. Vessels spawn near each other, near named bodies
   (low/high orbit) or phenomena (rings). Rings/asteroid fields are
   procedurally generated from a fixed seed when a ship is near. Max object
   count ≈ solar-system-sized dataset.
5. **Multiplayer**: server binary owns the simulation and renders text;
   clients are dumb terminals over TCP (see what the server sends, type
   lines back). Begin2 itself already had this shape (control type 2 =
   remote player — `sub_21A7C` scenario loader, 64518; `sub_20F6A` remote
   input, called from `XXSimulate+83`).
6. **In high orbit it plays exactly like begin** — combat accelerations
   dwarf gravity; the ephemeris only matters for scenery, stations, spawn
   anchoring, and long transits.
7. **The AI must be preserved.** Daniel spent years tuning it; Part III
   specifies it completely.

---

## Part II — The Simulation Engine (from the assembly)

### 1. Units, coordinates, angles

- Positions: doubles, X = node+34h, Y = node+3Ch (`distToTarget`, 33592).
- Bearing convention: degrees, 0° = +Y, 90° = +X (compass). Computed as
  `atan(dx/dy) × 57.2957795` with quadrant fix-ups (+180/+360)
  (`whichShieldFace` 33683, `sub_1340E` 33782). In Rust:
  `normalize(atan2(dx, dy).to_degrees())`.
- `mysteryDamFunc` (34297) = normalize angle to [0,360) (`fmod` then +360 if
  negative; 360.0 = `flt_33FF2`).
- `sub_1378D` (34186) = angular distance in [0,180]: `d = normalize(a−b);
  if d > 180 { 360 − d }`.
- `sub_1340E(a,b)` (33782) = bearing a→b **honoring last-known position**
  (§8); `sub_13535(a,b)` (33911) = reverse: course that points directly away
  from b. `sub_13108`/`sub_131B1` (33452/33525) = distance / distance²
  honoring last-known. `distToTarget` (33592) = true distance.
  `sub_1375C(a,b)` (34152) = **relative bearing (MARK)** =
  `normalize(bearing(a→b) − a.course)`. `sub_1372B(a,b)` (34118) =
  `normalize(bearing_away(a←b) − b.course)` = target's course angle off the
  line of sight (feeds torpedo lead).
- Speed: warp factor × 100 distance units per cycle. Movement integration:
  velocity vector per sub-step = `(sin, cos)(course°) × warp × 5.0`
  (`dbl_32DBA`=5.0; `sub_CEB8` 20451 tail); the ordnance loop runs the
  vector 19× per cycle interleaved with fuse checks (`detonation+56`,
  26446), the no-ordnance fast path multiplies by 20.0 (`sub_1032E` ≈26390,
  `dbl_332AE`=20.0). **Original-binary quirk: 19×5=95 vs fast-path 100 per
  cycle.** Rust port: use 20 sub-steps uniformly (document as intentional
  fix), keep `UNITS_PER_WARP_PER_CYCLE = 100`.
- RNG: `rand()/32767` uniform [0,1] (`randRangeHelper` 33390);
  `sub_1305E(lo,hi)` uniform double (33326); `randRange(lo,hi)` integer
  floor (33352); `percentRand(n)` = true with n% probability (33413).

### 2. Object model

Begin2 keeps a doubly-usable set of intrusive linked lists (heads: all
objects `378C9`, ships `378C1`, torps `378B9`+tail `378B5`, probes `378B1`).
Rust: one `Vec<Object>` arena with typed index lists, fixed capacity.

**OBJECT ("node") fields** (offsets for asm cross-reference):
`+0` type (1 ship, 2 torp, 3 probe); `+2` nation ptr; `+8`? ; `+0Ah`
ballistic flag (1 = never steers — unguided torps; `sub_E9B5` 23463);
`+0Ch` current warp (dq); `+14h` desired warp (dq); `+1Ch` accel applied
this cycle (dq); `+24h` current course (dq); `+2Ch` desired course (dq);
`+34h/+3Ch` X/Y; `+44h/+4Ch` velocity per sub-step; `+54h` power available
after warp (dq, "warp power" budget); `+5Ch` working power pool (dq, "other
power"); `+64h` residual power (dq, display); `+6Ch` detonation state (0
none, 1 expire quietly, 2 detonate live, 3 destroyed-by-damage); `+6Eh` helm
mode (0 course, 1 pursue, 2 elude); `+78h/7Ah` pursue-target ptr; `+7Ch`
owner ship (torps/probes); `+80h` → SHIP body; `+84h` → TORPSTATE; `+88h` →
PROBESTATE; `+8Ch` next-all; `+94h` next-in-type-list; `+9Ch` control type
(1 local human, 2 remote human, 3 AI, 4 none); `+9Eh/A0h` remote player name;
`+0A2h` currently-in-sensor-contact flag; `+0A4h` ever-detected flag;
`+0A6h/0AEh/0B6h/0BEh/0CCh` last-known X/Y/warp/course/cycle; `+0C6h` hull
integrity (single hit ≥ this destroys the ship; ≈ design mass).

**SHIP body fields**: `+0/+2` name ptr; `+4` survivors (int); `+22h` → AI
brain (0xA0 bytes, `sub_1D471` 55804); `+30h/+32h` reactors count/array
(stride 0Ah: {dmg% int, repair-progress dq}); `+82h/+84h` batteries
count/array (stride 12h: {dmg%, charge dq, progress dq}); `+114h/+116h`
phaser banks count/array (stride 2Ah: {dmg%, fire-flag, lock-target dword,
mark dq, charge dq, spread dq, state word (7 enabled/8 disabled),
progress dq}); `+266h/+268h` torpedo tubes count/array (stride 3Ah: {dmg%,
fire-flag, lock-target, mark dq, prox dq, …, +2Ah loaded TORPSTATE ptr,
+2Eh progress dq}); `+43Ah/+43Ch` probe launchers count/array (stride 1Ch:
{dmg%, fire-flag, +8 course dq, +10h loaded PROBESTATE ptr, +14h progress});
`+4ACh/+4AEh` warp drives count/array (stride 1Ah: {dmg%, temperature dq,
temp-delta dq, progress dq}); `+516h/+518h` shields count/array (stride 22h:
{dmg% int, hit-count word (report), hit-flag word, effective dq, strength%
dq, state word (7 up / 8 down / 9 reinforced), coverage bitmask word at +6
[VERIFY exact slot vs +1Ch], progress dq}); `+5E4h/+5E6h` hull-hit report
counters; `+5E8h` armour remaining; `+5EAh` shield-hits stat (32-bit);
`+5EEh` self-destruct countdown (dq, −1 = off); `+5F6h/+5F8h` scanner
dmg%/progress; `+600h` cloak capable+functional, `+602h` cloaked, `+604h/
+606h` cloak dmg%/progress; `+60Eh` → design; `+612h` boarders aboard,
`+614h/+616h` boarders' nation; `+618h/+61Ah` transporters count/array
(stride 0Ah {dmg%, progress}); `+66Ah` hits-taken-this-cycle (cloak reveal);
`+66Ch` tractor/dock partner node; `+674h/+670h` docked-ships list links;
`+678h` repair priority code (0 none; 1 launchers 2 banks 3 tubes 4 drives
5 shields 6 transporter 7 cloak 8 reactors 9 batteries 10 scanner 11 impulse
12 tractor — `sub_D72F` 21416); `+67Ch/+67Eh/+680h` impulse
exists/health/progress; `+688h/+68Ah/+68Ch/+68Eh` tractor
exists/engaged/health/progress; `+696h` tractor target; `+69Ah` tow bearing
(display); `+6A2h` docked flag; `+6A4h/+6A6h` group membership/number;
`+6A8h` max attainable warp (dq, cached); (see §12 AI brain).

**TORPSTATE** (tube+2Ah → node+84h after launch): `+0` warhead damage dq;
`+8` time-fuse/strength dq (dual use: plasma decay + phaser attrition);
`+10h` prox distance dq; `+20h` arm counter dq; `+28h` salvo count word;
`+2Ah` → TORPDESIGN. **PROBESTATE** (launcher+10h → node+88h): same through
`+20h`; `+2Dh` → PROBEDESIGN.

**Design structs**: field order and sizes in `shipeditor/shipdesign.h` /
`torpdesign.h` / `probedesign.h`; all instances in `stats200.h`. Key ship
design offsets used throughout the asm: mass +0Eh, max_warp +10h, w1accel
+18h, decel +20h, warp_power_use +28h, w1turn +30h, destruct +38h,
scanner_reflect +40h, reactor_output +4Ah, battery_capacity +5Ch, banks_rate
+6Eh, banks_charge +76h, banks_range +7Eh, tube_ids (torp design ptr) +90h,
probe_ids +0A2h, warp_power +0B4h [unused by decoded code], warp_efficiency
+0BCh, shield_strength +0CEh, shield_absorption +0D6h, shield_recharge
+0DEh, shield_energy +0E6h, beam_cap +0F8h, beam_range +0FAh (3000.1),
scanner_range +10Ah, can_cloak +11Ah, cloak_energy +11Ch, mass_capacity
+138h, docked_repair_ratio +140h, life_support +148h, tractor_strength
+156h, tractor_energy +160h, destruct_energy +168h. Repair rates: reactors
+52h, batteries +64h, banks +86h, tubes +94h, launchers +0A6h, drives +0C4h,
shields +0EEh, transporter +102h, scanner +112h, cloak +124h, impulse +14Ch,
tractor +158h.

**Nation dogma** (stats200.h lines 46–175): aggression, bravery, loyalty,
fanaticism, boarding combat level, deviation + crew names + endgame
messages. Federation .65/.95/1.20/.05/.5/.30; Klingon .95/.50/.60/.10/.5/.20;
Romulan .75/.95/1.00/.95/.5/.10; Orion .45×4/.75/.40.

### 3. The cycle (`XXSimulate` 5374)

```
loop {
  cycle_counter += 1
  side_strength_player = Σ sub_106F7(ship)  // sub_107E4 ≈27070
  side_strength_enemy  = Σ sub_106F7(ship)  // sub_1087C
  sensor_contact_update()                    // sub_E673 23114 (§8)
  for ship in ships { match control { Human => getInput, Remote => sub_20F6A,
                                       AI => ai_think(ship) /* sub_20D55 */ } }
  detonation_pipeline()                      // detonation 26446 (§4)
  if end_condition() { evaluation() }        // sub_1044E 26592, sub_104BB ≈26660
}
```
One cycle = one "second" of game time; all commands are entered between
cycles (turn-based input, real-time resolution — manual §II).

### 4. Resolution pipeline (`detonation` 26446) — order matters

1. `sub_C3B9` (19191): clear weapon locks & pursue targets pointing at
   objects with `+6Ch != 0` (dying), gated by dirty flag `word_32B0C`.
2. `sub_C4F7` (19337) per ship: power & drives (§5), life support, shields
   power/regen via `sub_C883` (19700, §6), cloak energy, tractor energy.
3. `sub_CA97` (19930): phaser charging (`sub_B1C1` 16697) then resolve all
   fire-flagged banks via `phaserDamage` (23256, §9.1).
4. `splash_starter` (21127): resolve detonation chain (§10.1).
5. `sub_CB5F` (20043): tube charging (`sub_B351` 16906 [same layout as bank
   charging]) + fire flagged tubes → spawn torpedo `sub_E9B5` (23463, §9.2).
6. `sub_CC27` (20154): fire flagged launchers → spawn probe `sub_ECDC`
   (≈23760).
7. `sub_CCE2` (20260): warp acceleration for every object (§5.1).
8. `sub_CEB8` (20451): pursue/elude re-aim, turning, homing guidance,
   velocity vector computation for every non-ballistic object (§5.2).
9. Movement: if any torp/probe exists, 19× { `sub_D0DC` (20667) integrate
   all objects; `sub_D2D2` (20900) prox fuses; `splash_starter` }; else
   `sub_1032E` (≈26390) single ×20 step.
10. `sub_D130` (20709): fuse bookkeeping once per cycle — decrement arm
    counter, then time fuse (torp expiry → `6Ch=1` fizzle; probe expiry →
    `6Ch=2` detonate; ship self-destruct countdown → `6Ch=2`).
11. `splash_starter`; `sub_D72F` (21416): **damage control / repair** (§11).
12. `sub_C010` (18795): boarding combat (§13.1).
13. `sub_102A7` (26336): auto-release tractor on detonating/docked targets;
    `sub_DE7C` (22201): battery recharge + residual power + brain battery
    fraction + towed-object position lock (glued to `+66Ch` partner).

### 5. Ship physics

**5.1 Warp change** (`sub_CCE2` 20260): desired clamped ≥ −1.0 (`dbl_32DA2`;
reverse to warp −1 possible) and ≤ max attainable (clamped earlier in
`sub_C4F7+68`). `diff = desired − current`. If `|diff| < 0.01`
(`dbl_32DAA`) → snap. Else if desired ≤ 1 and current ≤ 1 → **instant**.
Else rate = `+w1accel/current` if accelerating and current ≥ 1 (plain
`w1accel` below warp 1), `−decel` if decelerating; magnitude capped by
`|diff|`; `current += rate`.

**5.2 Turning & guidance** (`sub_CEB8` 20451): skip ballistic (`+0Ah`).
Pursue mode: desired course = bearing to target each cycle; elude: directly
away. `diff = normalize(desired − current)`; if object is not a ship, or
`current_warp ≤ 1`, or diff ≈ 0 → snap to desired. Else: if diff > 180 →
diff −= 360 (shorter way); turn = `±w1turn / current_warp` capped by |diff|;
`course = normalize(course + turn)`. Then velocity per §1.

**5.3 Max attainable warp** (`sub_EFB2` 24112): fallback = 1.0 if impulse
exists and healthy else 0. If warp-power budget (node+54h) = 0 → fallback.
Else `min(design.max_warp, node54h / warp_power_use)`, and if that ≤ 1 →
fallback. Cached at body+6A8h every cycle (`sub_C4F7+37`).

**5.4 Warp temperature** (`sub_F259` 24428) per drive, per cycle:
if drive destroyed (dmg=100): `temp −= warp_efficiency×4`, floor 0. Else:
`temp += (warp_ratio − health_frac × warp_efficiency) × 4` where
`warp_ratio = desired_warp / max_attainable` (passed from `sub_C4F7+116`),
`health_frac = (100−dmg%)/100`; floor 12.0 (`flt_32FCF`); **if temp > 40.0
(`flt_32FD3`) the drive is destroyed** (dmg=100) with report. Temp delta
stored for the ↑↓ display. Player low-battery/temperature warnings:
`sub_F530` 24766, `sub_F5A6` 24834.

### 6. Power & shields

**Power** (`sub_EED2` 24006 init; `sub_C4F7` 19337 consumption): each cycle
`node+54h = gross_power` (`sub_F07A` 24210 [VERIFY exact composition: sums
reactor output × health; also used as destruct-yield basis]); pool
`node+5Ch = Σ reactor_output × (100−dmg)/100 + Σ battery charge`. Warp
consumes `desired_warp × warp_power_use` from node+54h; `pool += 0.5 ×
node54h_残` [VERIFY exact split — see `sub_C4F7+116..124`; display fields:
WARP POWER = node+54h, OTHER POWER = pool, RESIDUAL = node+64h]. Life
support needs `crew/10` per cycle from an accumulator; on shortfall a
failure counter (brain+9Eh) increments — **3 consecutive failures kill the
whole crew** (`sub_F64C` 24917); docked ships exempt. Cloak upkeep
`cloak_energy` else decloak; tractor upkeep `tractor_energy` else release
(`sub_C4F7+270..`). End of cycle `sub_F101` (24276): pool refills batteries
to `battery_capacity` each; leftover = residual (node+64h); brain+94h =
average battery fill fraction.

**Shields** (`sub_C883` 19700): cost = Σ per non-down shield
`shield_energy` (×4.0 if reinforced, state 9). If pool < cost → **all
shields drop this cycle** (effective 0) and brain+9Ch=1 (power-starved flag
the AI reads); else pool −= cost. Regen per shield: ceiling = 100 − dmg%;
if strength < ceiling and not down: `strength += max(strength/100 ×
shield_recharge, 0.1)` (`dbl_32B48`), capped at ceiling. Effective =
strength if powered and not down else 0. Shield-face selection
(`sub_EE0A` 23888): relative bearing → face mask (±30° front=1, 30–90=2,
90–150=8, 150–210=0x20, 210–270=0x10, 270–330=4); first non-destroyed,
non-down shield whose coverage mask matches. Manual numbering: shield 1
front (0°), 2 = 60°, 3 = 300°, 4 = 120°, 5 = 240°, 6 = 180°.

### 7. Weapons

**7.1 Phasers.** Charging (`sub_B1C1` 16697): per enabled (state 7),
undamaged, non-firing bank: `charge += min(needed, banks_charge/banks_rate
(+0.001), pool)`; pool pays. Fire executors set flags only: `sub_AD74`
(16059) marks banks to fire with spread; `sub_BAD0` (17954) = AI helper
"fire first n fully-charged banks at spread s"; lock: `sub_BC4D` (18183) /
`sub_B092`; manual turn: `sub_AF38` (16314). Resolution (`phaserDamage`
23256), per firing bank: bank facing = `normalize(ship.course + bank.mark)`
(mark auto-tracks lock target — `sub_B875` 16xxx sets mark from lock);
for **every object**: skip self & cloaked; in range `dist < banks_range`
and in cone `angular_diff(bearing→obj, facing) ≤ spread/2`:
`damage = bank.charge × (45.0/spread) × sqrt(1 − dist/range) × 0.5`
(`phaserDamMult` 99290, `flt_32CBE` 99196) applied via
dealDamageToHull(obj, face, damage, type=4). Bank charge zeroed after fire.
Spread player-selectable 10–45°; AI uses 45 default, or `angle_off × 2`
when sniping (§12.5). Point defense burns down **decaying** torpedoes
(§10.3).

**7.2 Torpedoes.** Load (`sub_BD34` 18331 → `sub_B4BA`): allocates
TORPSTATE from tube's torp design: damage, prox (player/AI-set, ≤ design
max), arm time, time fuse. Lock with lead offset: `sub_BCBA` (18254) /
`sub_B107`. Firing solution (`sub_B926` 17715): homing → mark = relative
bearing; else `mark = normalize(rel_bearing + asin(sin(angle_off_rad) ×
target_warp / torp_velocity)·deg + lead_offset)` — true intercept lead.
Fire (`sub_BB62` 18037 → `sub_AE17` flags; spawn in `sub_E9B5` 23463):
course = ship.course + tube.mark; **velocity = design.velocity ±
speed_variance** (uniform); non-homing torps are ballistic (velocity vector
frozen); homing torps get pursue mode. **Salvo merge** (`sub_EC1D` ≈23680):
a torp fired the same cycle, same position/design/course/target as the head
of the torps list merges into it (`salvo_count += 1`) — splash applies
damage `salvo_count` times. Prox fuse: every movement sub-step, armed torps
detonate within `prox` of **any ship including friendlies** (`sub_D2D2`
20900). Time fuse expiry → fizzle (no blast).

**7.3 Probes.** Load `sub_BDB2` (18410 → `sub_B6A4`) with prox+time; launch
at target or course (`sub_BBD3` 18107 → `sub_AE9B`; spawn `sub_ECDC`
≈23760; owner recorded at node+7Ch). Slow homing weapons re-aiming every
cycle; controllable after launch (lock/turn/detonate — `sub_20AF8` 62435
for AI remote-detonate). Prox triggers only on other-nation ships (or a
same-nation ship it was deliberately locked on). Data probes (scan range >
0) extend the side's sensor coverage (§8). Expiry → detonates (`6Ch=2`).

**7.4 New weapons for the port (design).** Railgun: hitscan cone weapon
reusing the phaser pipeline with `spread` small (~1–2°), no charge decay
with distance until `range`, damage flat per slug, travel effectively
instantaneous at 0.01–0.1% c (30–300 km/s: cross a 20,000-unit engagement
in ≪1 cycle) — resolve same-cycle like phasers, but apply `dealDamage` with
type=antimatter (penetrates shields normally) and no splash. Kinetic
rounds: ballistic unguided "torpedoes" with prox=0 (contact), no splash
radius (or tiny), high velocity, cheap reload. Fighters: ships with tiny
designs launched/recovered like docked ships (§13.3) from a battlestar hull
(mass_capacity + dock rules already exist to support this).

### 8. Sensors & fog of war (`sub_E32F` ≈22800, `sub_E673` 23114)

Feature active only when the scenario enables it (`word_37806`; the
`Random` setup command spreads ships out — manual v2 notes). Player-side
objects are always "visible" (the original tracks fog only for the player's
benefit; the AI reads the same flags, so enemies with lost contact use
stale data too). Visibility test for an enemy object: reflectivity `r` =
probe → 0.5; ship → design.scanner_reflect; ×0.005 (`dbl_32EEE`) if cloaked
and not hit this cycle (being hit reveals — body+66Ah). Object is visible
if within `scanner_range × r` (per-axis box test, not circle) of any
player-side ship with working scanner, or within `probe_scan_range × r` of
any player-side data probe. Transitions produce crew messages ("contact" /
"lost contact", `sub_E54B`/`sub_E643` ≈22990); on loss, last-known
X/Y/warp/course/cycle recorded (node+0A6h..0CCh) and **all distance/bearing
math against that object uses the ghost** (`sub_13108`/`sub_1340E`).

### 9. Damage application (`dealDamageToHull` 25192)

Args: (target, face-angle, damage, type). Type: 4 = phaser, else torpedo
warhead enum; **type 1 = SHIELD_BORE** (Orion Auger). Damage int-rounded,
capped 8000 (`flt_3327A`).

Ships:
1. body+66Ah++ (cloak reveal); target brain torp-pressure −5 (floor 0).
2. Facing shield := `sub_EE0A(target, face)`. No shield → all to hull.
3. Shield hit: absorb capacity = `shield_strength × effective%/100 × 0.25`
   (`flt_3327E`); absorbed = min(damage, capacity) (SHIELD_BORE: damage ×4
   first). Shield strength loss = `1 + absorbed × (1 − shield_absorption ×
   0.25)`, halved if reinforced; effective clamped to strength. SHIELD_BORE
   stops here (never penetrates). Others: `damage −= absorbed`; if ≤ 0 done.
4. Hull: armour absorbs first (body+5E8h). If remaining damage ≥ hull
   integrity (node+0C6h) → **ship destroyed** (`6Ch=3`). Else crew
   casualties = `rand(0,dmg) + rand(0,dmg)`; survivors < 6 → all hands lost
   (`sub_F64C`). Boarders take `rand(0,dmg)`. Docked ships recursively take
   the same hit if dmg ≥ 10.
5. System damage: `n = dmg/10` rolls; each roll hits each class with
   independent chance — shields 75%, tubes 50%, banks 45%, drives 40%,
   reactors 60% (25% of those destroy the reactor outright, else double
   damage), launchers 20%, batteries 15%, transporters 10% (double), scanner
   10%, cloak 10% (+decloak), impulse 5%, tractor 5% (+release). Random item
   in class; `dmg% += (n)×5 + rand(0,25)` capped 100 (`sub_F820` 25155).
   Damaged banks/tubes lose charge & locks; a loaded torpedo in a damaged
   tube is destroyed.

Torpedoes (type-2 targets): only **decay-type** torps (plasma) are
damageable — remaining-strength field −= damage; ≤0 → fizzle. Probes:
similar (tail of `dealDamageToHull` 25990+, `loc_FFBD`).

### 10. Splash / detonation (`splashDamage` 22271, `splash_starter` 21127)

1. `splash_starter` loops until no `6Ch != 0` remain (chains: a destruct
   can destroy ships whose destructs then fire); `6Ch==1` removes quietly,
   else `splashDamage` then removal; afterwards batch hit reports (shield N
   hit / hull damage) to players.
2. Yield: **ship** = `gross_power/ooDestructDamage` (+ design.destruct if
   deliberate destruct `6Ch==2`); recursively detonates docked ships.
   **torp** = warhead damage (armed; else 0); decay-type scales by
   `remaining/max_time_fuse`. **probe** = warhead damage (armed or
   remote-detonated).
3. Blast: `total = yield × SplashDamMult (5.0)`; radius = `total × 10.0`
   (`flt_32EEA`); for every non-cloaked object within radius:
   `dmg = total × sqrt(1 − dist/radius)`, face = bearing-from-target,
   applied `salvo_count` times, type = warhead.
4. Ship self-destruct: `Destruct` sets countdown (body+5EEh) = 5 cycles
   (`flt_35F6E`; costs design.destruct_energy); "average ship destruct
   damage" default 9.0 tuned →14.0 (notes.txt).

### 11. Repair (`sub_D72F` 21416, kernel `sub_F6AE` 24949)

Per ship per cycle: efficiency `e` = docked ? host_design.docked_repair_ratio
: survivors/design.crew; skip if < 0.1. If a repair priority is set: that
class ×4, all others ×0.5. Per item: destroyed (100%) items are repairable
**only while docked**, then only passing a 2% per-cycle gate. Progress +=
`rand(0, 2 × class_rate × e)`; integer part reduces dmg%; at 0 → "repairs
completed" report (`sub_F781` 25053; 12 engineer names). Class rates from
the design struct (§2 table; stats200.h lines 16–22 macros).

### 12. Boarding, transporters, tractor, docking

**12.1 Boarding combat** (`sub_C010` 18795), per ship with boarders: 35%
skip if defenders ≥ 6. Rounds = `rand(0,1)×boarders`: attacker roll
`rand(0, attacker_nation.boarding_level)` vs defender roll; loser side
loses 1 (margin 0.1). Boarders wiped → "repelled". Defenders < 6: boarders
< 6 → mutual annihilation (derelict); else **capture**: ship's nation :=
boarders' nation, control := AI, survivors := boarders, fresh brain
(`sub_1D4BD` 55844), 10%/cycle chance to defuse a running self-destruct.

**12.2 Transporters** (`sub_16897` 41286 count, `sub_16927` ≈41360
validity, `sub_16A70` 41544 execute): max = min(Σ beam_cap of working
transporters, from.survivors − 6, to.life_support − to.survivors −
to.boarders); range = design.beam_range (3000.1); target must be alive, in
sensor contact, **shields down** (`sub_FFCC`), not docked (if enemy), and
existing boarders must be ours. Beaming crew onto an enemy = boarding.

**12.3 Tractor & docking**: engage = body+68Ah + target (AI: `sub_1D2BE`
55564). Energy upkeep per cycle. Pull physics (`sub_100B2` 26135): ratio =
`tractor_strength / target_design.mass`; scaled down by `max(1, dist/1000)`
(`flt_332AA`); < 0.1 → too weak, release; pull distance = `min(pull, dist)
× 5.0` per cycle added to **target's** velocity toward the ship. Once
docked/latched (body+66Ch partner set) the towed object's position is
copied from the partner each cycle (`sub_DE7C+4E`). Docking permission
(`sub_1623D` 40317): same nation, alive, host mass_capacity > 0, dist ≤
1000 (`flt_347C0`), Σ docked masses + ship mass ≤ capacity. Docked ships:
repair bonus, protected life support, recursive splash exposure.

### 13. Endgame (`sub_1044E` 26592, `sub_104BB` ≈26660)

Game ends on quit, player-ship destruction, or fleet elimination. Rating:
`score = (ally_losses_frac × odds − enemy_losses_frac / odds) /
ally_strength`-style formula (see asm) bucketed at >15, >5, >2, >0, >−2,
>−5, else → messages[1..7] from the nation dogma. Thresholds `flt_3339B..
dbl_333AF` (99355–99359).

---

## Part III — The AI (preserve exactly)

Everything below runs per AI ship per cycle. Globals set up by `sub_1E08A`
(57274): MY node/body/brain/design/nation; `dbl_38692` = my max attainable
warp; `word_38690` = can move; counts via `sub_BE37/BEAC/BF0D`
(18491/18556/18612): `word_3860C/word_3860A` = operational/charged banks,
`word_38608/word_38606` = operational/loaded tubes, `word_38604/word_38602`
= operational/loaded launchers; caches: torp design + max prox
(`dbl_385FA`), probe design + velocity (`dbl_385EA`) + max range
`dbl_3860E = (arm+max_time) × velocity × 100`, bank range `dbl_38626`;
brain steering cooldown +1Ch increments; **nearest enemy heading at me
within ±10°** → `dword_38646` + distance `dbl_3863E`. After target
selection, `sub_1E42D` (57627) caches: distance `dbl_38674`, bearing
`dbl_3866C`, target body `dword_3867C`, target's incoming-torp pressure
`word_3865A`, target max warp `dbl_38664`, target bank/tube counts
(`word_3865E`/`word_38662`), my torp min/max range vs target
(`dbl_3861E`/`dbl_38616`, via `sub_1D7A3` 56151: `closing = torp_vel +
target_warp × cos(rel_angle)`, `min = arm_time × closing × 100`,
`max = (arm+max_fuse) × closing × 100`), weapon-dominance ratios
`dbl_38636` (phaser) / `dbl_3862E` (torp) = my score / target score (1000
if target has none; from strength components brain+4Ah/+52h).

**Ship strength estimate** (`sub_106F7` ≈26906): `1 + charged_banks ×
banks_range/1000 + loaded_tubes × 6`; brain +4Ah/+52h/+5Ah; +62h = value at
spawn (max).

### 12.1 Personality (`sub_1D4BD` 55844)

At spawn (and on capture): per-attribute `clamp01(nation_value +
uniform(−deviation, +deviation))` for bravery (+72h), loyalty (+7Ah),
fanaticism (+82h), aggression (+6Ah); weave side (+36h) = ±1 with 50%;
stance (+1Ah)=0; mission (+6)=0; last-prox (+3Ah)=−1; steering cooldown
(+1Ch)=2.

Brain field map (for asm cross-ref): +0 target; +4 target-was-ordered flag;
+6 mission code; +8 mission target ptr; +0Ch second target (tow dest);
+10h mission value dq (range/count/course); +18h last mission (for
announcement dedup); +1Ah stance (0 normal, 2 retreating, 0x15 destruct-
ram); +1Ch steering cooldown; +1Eh weave base course dq; +2Eh weave
amplitude dq; +36h weave side ±1; +38h weave mode (1 toward, 2 away); +3Ah
last prox ordered; +42h [gates sub_20AF8 — VERIFY]; +46h overheat-slowdown
latch; +48h retreat announced; +4Ah/+52h/+5Ah/+62h strength components;
+6Ah aggression; +72h bravery; +7Ah loyalty; +82h fanaticism; +8Ah incoming
torp pressure (0–90); +8Ch last tube lead offset dq; +94h battery fill
fraction; +9Ch shields-power-starved flag; +9Eh life-support failure count.

### 12.2 Dispatcher (`sub_20D55` 62774) — priority order

```
setup_context()                                    // sub_1E08A
if reflex_block(): return                          // sub_20C86 62632:
   sub_1E59D drive-overheat slowdown → sub_1E6AC flee detonation
   → sub_1E78A snap phasers → sub_1E8B8 point defense
   → sub_1E9BB reinforce weakest shield;  (else) sub_1EA71 un-reinforce
select_target()                                    // sub_1E30D 57502
if target: cache_target_context()                  // sub_1E42D
if brain+42h==0: sub_20AF8 remote-detonate ordnance → return if acted
if weapons_block(): return                         // sub_20D07 62718:
   sub_1EAEF fire phasers at target → sub_1EC17 fire torpedoes
   → sub_1EE1E launch probes → sub_20C3C board via transporter
if sub_20BF2 cloak-when-idle: return               // 62550
if brain+6 (mission): sub_1F3B8 mission executor   // 59564
elif stance: sub_1EF17 stance helm                 // 58970
else { sub_1EFC8 morale check; sub_2001A default combat maneuver } // 59066/61052
if nothing acted: sub_1F09F lock/load housekeeping // 59195
```

### 12.3 Reflexes

- **Overheat slowdown** (`sub_1E59D` 57779): scan drives; if any damaged
  drive and max temperature > 38.0 (`flt_35FF8`) and
  `(100−maxdmg)/(100−mindmg) < 0.25` → set desired warp 1.0 (cool), latch
  +46h.
- **Flee imminent detonation** (`sub_1E6AC` 57904): any object with
  destruct countdown in (0,5] whose blast radius (`sub_1D92D` 56325: ship →
  `(gross_power/9 + design.destruct) × 50`; probe → `warhead × 50`) covers
  me → course directly away, full combat speed.
- **Snap phasers** (`sub_1E78A` 58013): needs charged banks and the nearest
  approaching enemy (`38646`) inside bank range. Find a charged bank that
  bears (`sub_1DCBC` 56795: |bank facing − bearing| < 22.5°). If the
  target's facing shield is up: spread = `angle_off × 2`; expected damage =
  `charge × sqrt(1−d/r) × (45/spread) × 0.5` (`sub_1DDB8` 56914); fire
  `ceil(shield_effective / expected)` banks (capped at charged) — exactly
  enough to break the shield. Shield down: spread 45, one bank.
- **Point defense** (`sub_1E8B8` 58165 → `sub_20697` 61943): nearest
  hostile probe within `(v_me+v_thr)×100×2.5 + bank_range`; skipped when a
  ship target is in phaser range and phasers dominate. If no charged bank
  bears → rotate one bank to the threat's mark (`sub_AF38`); if threat
  beyond range → desired warp −2 (min 1) to let it close; else fire that
  bank at spread 45.
- **Shield reinforcement** (`sub_1E9BB` 58292): when power-starved flag off
  [VERIFY polarity], reinforce (state 9) the weakest damaged shield;
  (`sub_1EA71` 58384) drop reinforcement (state 7) when residual power <
  shield_energy.

### 12.4 Target selection (`sub_207F9` 62096; chooser `sub_1E30D` 57502)

Highest score wins; ties keep earlier. Score for candidate C:
- 0 if never detected (+0A4h==0) or same nation. Dead hulk: 0 unless it's
  my probe-mission target (keep = HUGE).
- Mission 22 (defend D): dist(D,C) < 1 → HUGE; > 50000 → 0; else `1/dist`.
- If C == ordered target (brain+4): obey with probability `loyalty×2 > rand`
  → HUGE.
- Base = C's strength estimate (brain+5Ah). Multipliers: distance band
  table ds:0xA65C (raw dump asm 101584–101659) — bands of 2000 units:
  ×3.0, ×1.9, ×1.75, ×1.5, ×1.4, ×1.3, ×1.2, ×1.15, ×1.125, ×1.1; ≥20000 →
  ×0.5. ×2.0 if within my bank range with operational banks. ×1.25 if C is
  my current target (hysteresis). ×0.5 if my probes already chase C
  (`sub_1D9B1` 56401 counts probes pursuing C). ×0.5 if C already
  outnumbered: `sub_1D86B` (56229) share = C.strength / Σ strengths of
  ships targeting C; if `(1−bravery)×5 > share`. ×0.1 if sensor contact
  stale (+0A2h==0).
- On target change: announce, clear ordered flag, cancel weapon-orders
  missions 4/5/6.

### 12.5 Fire control

- **Phasers at target** (`sub_1EAEF` 58464): needs operational banks,
  dist ≤ bank range, target alive. If banks not all locked on target →
  lock all (act). Else if charged: friendly-fire check first (below);
  volley: `rand+0.1 < aggression` → fire `(charged+1)/2`; else fire all
  **only if** all operational banks are charged. Spread 45.
- **Torpedoes** (`sub_1EC17` 58601): needs loaded tubes, min ≤ dist ≤ max
  torp range, target alive. Lead offset = 20° (`flt_36004`) if target
  pressure > 80 and torp not homing, else 0; ensure tubes locked with that
  offset (`sub_1F2C6` 59443). Fire when tubes bear: tolerance = `(offset +
  12)/2` degrees and friendly-corridor clear (`sub_20398` 61531: no allied
  ship within [homing ? dist×1.1 : max_range] whose bearing is within
  tolerance of the firing solution; sets `word_38684` when blocked).
  Volley (`sub_2047A` 61646): 10% hold; 1 if single tube or homing torp;
  target slow (<2.0 warp) or pressure < 30 → ALL; pressure > 60 → 35%
  chance 1 else hold; else (30–60) → 10% hold else half; capped by loaded.
  Fired count is added to the **target's** pressure counter (cap 90).
- **Probes** (`sub_1EE1E` 58840 → `sub_1ED89` 58770): needs loaded
  launchers and dist ≤ probe range (÷3 for probe-mission). If not all
  loaded → load (prox = 1000 when retreating else 100 [VERIFY mapping],
  time = design max). Mission 6: launch if no probes already chasing.
  Mission 2 (attack order): launch if no other AI engages the target,
  pursuers ≤ aggression×6, 50% gate. No mission: only if target slower
  than the probe, target has no charged banks, no probes chasing.
- **Remote detonation** (`sub_20AF8` 62435): my probe pursuing a target
  that now outruns it, target inside blast radius, me outside → detonate.
- **Housekeeping** (`sub_1F09F` 59195): lock all tubes → else lock all
  banks → else set prox = `target_max_warp ≤ 2 ? 100 : min(target_max_warp
  × 50, design max)`; reload tubes when prox order changes.

### 12.6 Maneuver

- **Default** (`sub_2001A` 61052): immobile → nothing; docked → stop; no
  target → wander (`sub_1F200` 59343: allies of the player hold position;
  enemies: 5% keep course; else 50/50 turn ±90°, cruise warp `sub_218C7`
  64248 = `max_attainable × (100−maxtemp?)/100 × warp_efficiency`
  [see asm — uses drive damage%]); if my probes chase the target →
  approach mode 2; elif boarding favorable (`sub_1E018` 57219: my survivors
  ≥ target's × (1−aggression) × 10 and transportable > 10) or phasers
  dominate (`sub_1DF9C` 57137) → phaser-range maneuver; elif torps dominate
  (`sub_1DFDA` 57178) → torp-range maneuver; else full stop.
- **Phaser range** (`sub_20120` 61222): inside bank range: if my facing
  shield toward the threat is nearly down (`sub_20A8F` 62377: effective <
  10) → random jink (`sub_2034B` 61490: course rand(0,360), cruise warp);
  else pursue target at `target_warp + 1`. Far outside (> range×2):
  approach mode 1. Just outside: pursue at `target_warp + aggression×4`.
- **Torpedo range** (`sub_201C7` 61310): if friendly blocked the corridor:
  force-steer bearing ± 90° (50/50). Else standoff = `min_range +
  (1−aggression) × (max_range − min_range)`; too close → mode 2 (extend),
  else mode 1 (approach).
- **Weave executor** (`sub_2029B` 61410; every ≥2 cycles): base = bearing
  (mode 1) or bearing+180 (mode 2); amplitude (`sub_2051F` 61763) =
  `rand(0, (1.1 − aggression) × (31 − closest_threat_dist/833.3))` when a
  threat is within 25000, else 0; side flips each update; course =
  `normalize(base + side × amplitude)`. Warp (`sub_205DF` 61853): from
  `sub_1DE53` (56985): 1.0 (= crawl) if batteries < 25% or max warp ≤ 1;
  else `maxwarp × (100−maxdrive_dmg)/100 × warp_efficiency`, halved while
  temperature-panicked (enter at normalized temp (T−12)/28 > 0.85, exit
  < 0.4, latch +46h); then reduced further if the turn needed exceeds
  `w1turn/warp × 2` (slow to turn); floor warp 1.
- **Stances** (`sub_1EF17` 58970): 0x15 destruct-ram → pursue target at max
  warp (no target → full stop). 2 retreat → if recovered (damage ratio <
  bravery×0.5, `sub_1DB12` 56572) resume; else run: course = bearing,
  mode 2.

### 12.7 Morale (`sub_1EFC8` 59066)

- Retreat check (`sub_1DB78` 56631): not docked, has target, mobile, target
  within `(1.2 − bravery) × 100000`; retreat if undamaged? no → if damage
  ratio (1 − strength/max) > `bravery × 0.75`. On retreat: prefer a
  dockable friendly base (`sub_1623D` returns 1/100) → announce
  "retreating to X"; else stance 2 + one of 8 retreat lines (table
  ds:0xA628, strings asm 101663–101674; repeat = "continuing retreat").
- Last stand (`sub_1DA17` 56460): if boarders outnumber crew — or crew <
  `(fanaticism+1) × 6` and `rand ≥ bravery` — or (fanaticism > 0.9 and
  target inside my blast radius): **set self-destruct** (`sub_1DC5E` 56752;
  4 announcement lines, ds:0xA618, 50% chance the player's science officer
  detects it) and stance 0x15 (ram).

### 12.8 Player orders — missions (`sub_1F3B8` 59564; jump table 61462)

Ally sanity: allies (player-nation AI) disobey with probability
`rand ≥ loyalty×2` (`sub_200BD` 61163) → random insult (5 lines, ds:0xA648)
and mission cleared. Docked ships refuse everything except undock (and
tractor-release variants). Mission codes ↔ `Tell <ally> …` commands:

| # | Order | Behavior (asm case at 59564+) |
|---|---|---|
| 1 | escort <ship> <range> | speed-match when `|dist−range| < 500`; else pursue at `target_warp × (dist>range ? 1.5ish : 0.5)` [see case 0x0] |
| 2 | attack <ship> | approach mode 2 (weave in) |
| 3 | course <c> | steer c, mode 1 |
| 4 | phaser <ship> | phaser-range maneuver (needs charged banks) |
| 5 | torpedo <ship> | torp-range maneuver (needs tubes) |
| 6 | probe <ship> | steer to bearing; probes released by §12.5; evade if probes chasing |
| 7 | standoff/withdraw | keep `(1−aggr)×10000 + 20000 ± 5000`; 33% jink between |
| 8 | transport <n> <ship> | beam n crew (validity §12.2), decrement remaining |
| 9 | dock <base> | `sub_16373` seek + dock |
| 10 | undock | `sub_16140` |
| 11 | tow <ship> <dest> | tractor (ratio = strength/mass must be ≥1), drag to dest at warp=ratio, release within 1000 |
| 12 | release | tractor off |
| 14 | recover <ship> | base recovers ship (`sub_16579`) |
| 15 | eject <ship> | base ejects (`sub_1662B`) |
| 16 | approach <ship> <range> | station-keep: `sub_1D342` 55638 — inside range: match target course+warp; > range×1.5: pursue at target_warp+2; between: steer bearing mode 1 |
| 17 | tractor <ship> | engage tractor (range 1000) |
| 18 | stop | hold course, warp 0 |
| 19/20 | join/leave <group> | set/clear body+6A4h/+6A6h |
| 22 | defend <ship> | stay within 3000 of ward; targeting biased to ward's attackers (§12.4) |

### 12.9 AI constants (all values verified in dseg; asm lines 99161–102478)

| Constant | Value | Use |
|---|---|---|
| flt_35CBC 101660 | 1.5 | station-keep outer hysteresis |
| flt_35CC0 101661 | 2.0 | pursue warp bonus; loyalty scale; in-range score bonus; PD decel step |
| flt_35F05 101685 | 0.5 | far/piling/outnumbered score penalties; phaser 0.5 factor twin |
| flt_35F11 101688 | 100.0 | units per warp-cycle; percent divisor |
| flt_35F15/ooDestructDamage | 9.0 | destruct yield divisor (**tuned 14.0**) |
| flt_35F19 101691 | 50.0 | blast-radius estimate & prox scaling |
| flt_35F1D 101693 | 6.0 | fanatic crew threshold; probe-swarm limit |
| dbl_35F44 101696 | 0.9 | fanaticism ram threshold |
| dbl_35F4C/flt_35F54 | 1.2 / 1e5 | retreat consideration range |
| flt_35F58 101700 | 0.75 | retreat damage threshold (×bravery) |
| flt_35F6E 101702 | 5.0 | destruct countdown; overkill scale |
| flt_35FA5 101705 | 22.5 | bank-bears cone |
| flt_35FA9 101706 | 45.0 | default spread (**16.3125 for begin1 feel**) |
| flt_35FAD 101708 | 0.25 | drive-damage ratio; battery-crawl threshold |
| flt_35FB1/35FB5 | 12 / 28 | temp normalize (T−12)/28 |
| dbl_35FB9/35FC1 | 0.85 / 0.4 | temp panic enter/exit |
| flt_35FC9 101715 | 10.0 | approach cone; boarding min; PD near |
| flt_35FF4 101720 | 1000.0 | tractor/tow radius; "infinite" ratio |
| flt_35FF8 101722 | 38.0 | overheat reflex |
| dbl_35FFC 101723 | 0.1 | aggression fire bias; stale-contact ×0.1 |
| flt_36004 101725 | 20.0 | jinking-target lead offset |
| dbl_36008 101726 | 1.1 | homing corridor factor; weave (1.1−aggr) |
| flt_36010 101728 | 3.0 | probe-mission range divisor |
| flt_36053/dbl_36057 | ±90.0 | wander/sidestep turn |
| flt_360C8 101739 | 3000.0 | defend radius |
| flt_3654B/3654F/36553 | 1e4/2e4/5e3 | standoff mission envelope |
| flt_36580 102462 | 500.0 | escort tolerance |
| flt_36640 102469 | 4.0 | approach warp bonus ×aggression |
| flt_36644 102470 | 360.0 | jink course range |
| flt_36648/dbl_3664C/flt_36654 | 25000 / 833.33 / 31 | weave amplitude model |
| flt_36658 102474 | 180.0 | extend = reverse |
| flt_3665C 102475 | 2.5 | PD window factor |
| flt_36660 102476 | 50000.0 | defend scoring radius |
| flt_36664 102477 | 2000.0 | score band width |
| flt_36668 102478 | 1.25 | target stickiness |

Combat-engine constants: `phaserDamMult` 45.0 (99290), `flt_32FC7` 2.0
(99288), `flt_32CBE` 0.5 (99196), `SplashDamMult` 5.0 (99276), `flt_32EEA`
10.0 (99277), `dbl_32EEE` 0.005 (99278), `flt_3327A` 8000 (99325),
`flt_3327E` 0.25 (99326), `flt_32FCF` 12.0 / `flt_32FD3` 40.0
(99291/99293), `flt_32D40` 4.0 (99201), `flt_32D53` 100.0 (99203),
`dbl_32B48` 0.1 (99180), `dbl_32DA2` −1.0, `dbl_32DAA` 0.01, `dbl_32DBA`
5.0, `dbl_32DC2` deg→rad (99208–99215), `dbl_332AE` 20.0 (99330),
`flt_332AA` 1000 (99329), `flt_347C0` 1000 (100261), `dbl_32AF4` 0.001
(99161), endgame `flt_3339B..dbl_333AF` = 15/5/2/−2/−5 (99355–59),
strength divisor `flt_333DD` 1000 (99398).

### 12.10 Crew chatter

All AI/crew messages route through `sub_11D24`/`sub_13D60` with dseg format
strings. Message pointer tables: destruct 4× ds:0xA618, retreat 8×
ds:0xA628, insubordination 5× ds:0xA648 (dump at asm 101663–101682). Port:
data-file of message templates keyed by event, per-nation crew-name rosters
from stats200.h.

---

## Part IV — Remaining `[VERIFY]` items (all minor, with dig sites)

1. Exact node+54h/+5Ch/+64h bookkeeping split (WARP/OTHER/RESIDUAL POWER
   display) — re-read `sub_C4F7` 19337 lines 19390–19470 and `sub_EED2`
   24006. Behavior-level model in §6 is close enough to start.
2. `sub_F07A` (24210) gross-power composition (destruct yield basis).
3. Shield coverage-mask assignment at ship creation (search writer of
   shield+6; likely in ship-instantiation `sub_5D9B` 5938-region /
   `sub_5844` setup).
4. `brain+42h` gate before `sub_20AF8` (set where?).
5. Probe load prox values in `sub_1ED89` 58770 (1000 vs 100 mapping).
6. `sub_218C7` cruise-warp damage term: drive damage% vs temperature —
   re-read 64248–64300 (`sub_21853` 64150-ish supplies the % pair).
7. Escort (mission 1) warp multipliers — case 0x0 at 59564+0xA56.
8. `.sce` scenario details beyond ship records (`sub_21A7C` 64518): header
   magic check, record types 4/6/7/8 (4 = player nation, 6 = remote player
   name, 7 = ship record: 0x22-byte fixed + nation + class + name strings +
   X/Y doubles + control word (−1 local human, −2 AI, else remote player
   index), 8 = end).
9. Phaser bank mark auto-tracking of locked targets (`sub_B875` ≈17660) and
   `getOrderInput`/command-parser tables (62887+, `seg005/seg006` command
   handlers, `What_Ship_To_Lock` 28313 name-resolution semantics: self-lock
   forbidden, ally warnings, flag bits 0x1/0x22/0x44 controlling permitted
   target classes) — needed only for exact command-line UX parity.

---

## Part V — Rust Implementation Plan

### Workspace

```
begin-rs/
  Cargo.toml            (workspace)
  crates/
    begin-core/         simulation library (no I/O)
      src/
        math.rs         bearings, normalize, angular_diff, rng (§1)
        data/           design structs + RON loaders
          ships.ron     ported stats200.h (all 4 nations, all classes)
          torps.ron     mk7/mk8/ktx/plasma/xplasma/opbt/oshb
          probes.ron    px2/pxd2/klpp/kdat/dnk/rmpp/rdat
          nations.ron   dogma + crew rosters + endgame messages
          messages.ron  crew chatter templates (§12.10)
          nearfuture.ron battlestar, fighters, railguns, kinetic rounds
        object.rs       Object arena, Ship, TorpState, ProbeState (§2)
        systems/
          power.rs      §6 (reactors, batteries, pool, life support)
          drives.rs     §5.3–5.4 (max warp, temperature)
          helm.rs       §5.1–5.2 (accel, turn, pursue/elude, velocity)
          shields.rs    §6 (regen, faces, reinforcement)
          phasers.rs    §7.1 + railgun variant (§7.4)
          torpedoes.rs  §7.2 + kinetic rounds
          probes.rs     §7.3
          damage.rs     §9 dealDamageToHull, §10 splash
          repair.rs     §11
          boarding.rs   §12.1–12.2 (combat, transporters)
          tractor.rs    §12.3
          sensors.rs    §8
        ai/
          brain.rs      personality, brain state (§12.1)
          context.rs    per-cycle globals (§ III intro)
          reflexes.rs   §12.3
          targeting.rs  §12.4
          gunnery.rs    §12.5
          maneuver.rs   §12.6 (weave, ranges, stances)
          morale.rs     §12.7
          missions.rs   §12.8
        cycle.rs        §3–4 pipeline (exact order!)
        scenario.rs     spawn config (TOML), classic .sce importer optional
        env/            (self-contained; small)
          mod.rs        Body table, max-objects cap
          spice.rs      anise + de440.bsp + moons/Ceres (feature "spice")
          kepler.rs     JPL approximate elements fallback (built-in tables)
          stations.rs   attach-to-body at low/geosync orbit
          procgen.rs    seeded rings/asteroid fields, activated by proximity
        plane.rs        3D adaptation layer (below) + planar-lock mode
    begin-server/       tokio TCP server, ANSI renderer
      src/
        main.rs         scenario load, tick loop (configurable cycle pacing)
        protocol.rs     line-based: client sends command lines; server
                        sends framed ANSI screen updates (dumb terminal)
        render/
          chart.rs      WARP COURSE BEARING RANGE MARK CLASS block
          scope.rs      position display + scanning range
          status.rs     SYSTEMS STATUS panel, damage report, shield/tube/
                        bank/launcher/probe status screens (manual pp.8–16)
          log.rs        colored crew-message feed
        commands.rs     parser: full v1.65+v2 command set (manual §VI +
                        v2 notes) + new: MARK syntax (course^mark), RAIL...,
                        LAUNCH FIGHTERS, PLANARLOCK (admin)
    begin-client/       dumb terminal: connect, raw mode, print frames,
                        send lines (rustyline-style local editing)
```

### 3D adaptation (design decisions)

- State gains Z, velocity gains Z; course stays compass-in-plane, add
  `mark ∈ [−90,+90]` (elevation). Velocity = `(sin c · cos m, cos c ·
  cos m, sin m) × warp × 5` per sub-step.
- All 2D formulas that use `bearing/angular_diff` generalize to the 3D
  angle between vectors for cones (phasers/railguns) and to
  (course, mark) pairs for helm; `whichShieldFace` uses in-plane bearing
  for faces 1–6 (adequate while combat is mostly planar; a top/bottom
  shield pair is a data-driven option later).
- Pursue/elude/torpedo lead work on full 3D vectors (the asin lead formula
  generalizes: aim = intercept solution in the plane containing shooter,
  target, and target velocity).
- Ships spawn with mark 0 on a shared plane. AI keeps its 2D logic for
  course and adds a mark-matching term in pursue (chase out-of-plane
  targets by setting mark toward them) — this preserves tuned behavior
  in-plane exactly, and behaves sensibly out-of-plane.
- `planar_lock = true` server option: Z and mark forced to 0 (spec
  requirement if out-of-plane dodging breaks balance).

### Fidelity rules

1. Implement formulas exactly as Part II/III, constants in one
   `constants.rs` mirroring §12.9 names, each with its asm line in a
   comment. Daniel's tuned values (14.0 destruct; optional 16.3125/0.3625
   "begin1 mode") behind a config profile.
2. Keep the pipeline order of §4 exactly — AI decisions depend on it
   (e.g., counts computed before firing, pressure decay on hit).
3. Same-cycle semantics: player/AI commands only set flags; resolution in
   the pipeline.
4. Random rolls: keep distribution shapes (uniform, percent gates).
   Seedable RNG for tests.

### Verification plan (task #7)

- Unit tests per system with known cases: helm convergence times vs
  hand-computed w1accel/w1turn; phaser damage at d=0, d=r/2; shield break
  bank-count matches §12.3 math; torp lead hits a constant-velocity target;
  temperature kills a drive at 40; repair completes in expected cycles.
- AI harness: 1v1 HC vs BC over 500 cycles — assert the AI locks, closes
  to standoff, fires, retreats when damaged past bravery threshold
  (deterministic seed).
- End-to-end: launch server with `duel` scenario, scripted TCP client
  drives `helm`, `lock`, `fire all tubes`, `damage`; battle resolves;
  compare crew-message sequence sanity. Optional golden run vs dosbox
  begin2 (begin2/dosbox-0.65 is in-repo) for qualitative parity.

### Milestones (post-approval)

1. `begin-core` skeleton: math, arena, RON data (classic ships), cycle
   shell, helm+power+drives → test: two ships fly.
2. Weapons + damage + shields + repair → test: scripted battle resolves.
3. AI (reflexes→targeting→gunnery→maneuver→morale→missions) → test: AI
   duel is competitive and "feels like begin".
4. Server/client + renderer + full command parser → playable multiplayer.
5. Environment (kepler fallback first, anise behind feature), stations,
   spawn anchors, procedural rings; 3D mark + planar lock.
6. Near-future content (battlestar/fighters/railguns) as data + the two
   small code paths in §7.4.

**Status: research complete. Nothing implemented yet. Awaiting Daniel's
go-ahead on the plan above (and on the 3D/shield-face and 20-substep
decisions called out inline).**

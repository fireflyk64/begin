#!/usr/bin/env python3
"""Extract all game data from begin2.exe into JSON data files for the Rust port.

Ship design records are located by searching for the double 3000.1 (beam_range,
at struct offset +0xFA — see AI_AND_COMBAT.md §2). Nation, torpedo and probe
design structs are reached through far pointers inside the ship records.
Far pointers are {offset:u16, segment:u16}; file offset = MZ header size + seg*16 + off.

Validated against the v1.65 manual examples and AI_AND_COMBAT.md:
- Federation dogma .65/.95/1.20/.05/.5/.30 (spec §2)
- mk8 "slower but 50% more punch" vs mk7 (v2 release notes)
- oshb warhead type 1 = SHIELD_BORE (spec §9)
- data probes (pxd2/kdat/rdat) scan_range 20000, "travel at warp 20" (v2 notes)
- tube charge time 3 cycles for mk7 (manual systems-status legend)
- distance-band table 3.0/1.9/1.75/... at ds:0xA65C (spec §12.4)

If begin.exe (v1.65) sits next to begin2.exe, the v1.65 weapon mount counts
(banks/tubes/launchers) override the begin2 ones — begin 2.00 halved most
loadouts (Klingon BC 5 banks/5 tubes -> 3/2) and the 1.65 counts are the
classic feel (the begin2 manual itself still describes the 5-bank Klingon
Frigate). v1.65 records: stride 0x166, located via the far pointer to the
class-name string; banks +0x64, tubes +0x86, launchers +0xA4, nation
name via *(rec+8).

Usage: python3 tools/extract_stats.py <begin2.exe> <output-dir>
"""
import struct, json, sys, os

exe = sys.argv[1] if len(sys.argv) > 1 else 'begin2.exe'
outdir = sys.argv[2] if len(sys.argv) > 2 else 'data'
data = open(exe, 'rb').read()
image_base = struct.unpack_from('<H', data, 8)[0] * 16
DSEG = image_base + 0x2b61 * 16

def rd(off):  return struct.unpack_from('<d', data, off)[0]
def rw(off):  return struct.unpack_from('<H', data, off)[0]
def farptr(off):
    o, s = struct.unpack_from('<HH', data, off)
    if o == 0 and s == 0: return None
    return image_base + s * 16 + o
def cstr(off, maxlen=200):
    if off is None: return None
    e = data.find(b'\0', off, off + maxlen)
    if e < 0: return None
    try: return data[off:e].decode('ascii')
    except UnicodeDecodeError: return None

# ---------- ship records ----------
anchor = struct.pack('<d', 3000.1)
bases = []
i = 0
while True:
    i = data.find(anchor, i)
    if i < 0: break
    bases.append(i - 0xFA)
    i += 1
bases.sort()
assert len(bases) == 20, f"expected 20 ship records, got {len(bases)}"

def name_list(ptr, count):
    """array of far pointers to strings; entry 0 is typically null"""
    out = []
    for k in range(count):
        s = cstr(farptr(ptr + 4 * k))
        if s: out.append(s)
    return out

def decode_ship(b):
    return {
        'name': cstr(farptr(b)),
        'abbrev': cstr(farptr(b + 4)),
        '_nation_ptr': farptr(b + 8),
        'crew': rw(b + 0x0C),
        'mass': rw(b + 0x0E),
        'max_warp': rd(b + 0x10),
        'w1accel': rd(b + 0x18),
        'decel': rd(b + 0x20),
        'warp_power_use': rd(b + 0x28),
        'w1turn': rd(b + 0x30),
        'destruct': rd(b + 0x38),
        'scanner_reflect': rd(b + 0x40),
        'reactors': rw(b + 0x48),
        'reactor_output': rd(b + 0x4A),
        'reactor_repair': rd(b + 0x52),
        'batteries': rw(b + 0x5A),
        'battery_capacity': rd(b + 0x5C),
        'battery_repair': rd(b + 0x64),
        'banks': rw(b + 0x6C),
        'banks_rate': rd(b + 0x6E),
        'banks_charge': rd(b + 0x76),
        'banks_range': rd(b + 0x7E),
        'banks_repair': rd(b + 0x86),
        'tubes': rw(b + 0x8E),
        'torp': cstr(farptr(farptr(b + 0x90))) if farptr(b + 0x90) else None,
        'tube_repair': rd(b + 0x94),
        'launchers': rw(b + 0xA0),
        'probe': cstr(farptr(farptr(b + 0xA2))) if farptr(b + 0xA2) else None,
        'probe_repair': rd(b + 0xA6),
        'drives': rw(b + 0xB2),
        'warp_power': rd(b + 0xB4),
        'warp_efficiency': rd(b + 0xBC),
        'drive_repair': rd(b + 0xC4),
        'shields': rw(b + 0xCC),
        'shield_strength': rd(b + 0xCE),
        'shield_absorption': rd(b + 0xD6),
        'shield_recharge': rd(b + 0xDE),
        'shield_energy': rd(b + 0xE6),
        'shield_repair': rd(b + 0xEE),
        'transporters': rw(b + 0xF6),
        'beam_cap': rw(b + 0xF8),
        'beam_range': rd(b + 0xFA),
        'transporter_repair': rd(b + 0x102),
        'scanner_range': rd(b + 0x10A),
        'scanner_repair': rd(b + 0x112),
        'can_cloak': rw(b + 0x11A) != 0,
        'cloak_energy': rd(b + 0x11C),
        'cloak_repair': rd(b + 0x124),
        'ship_names': name_list(farptr(b + 0x12C), 12),
        'captain_names': name_list(farptr(b + 0x130), 12),
        'mass_capacity': rd(b + 0x138),
        'docked_repair_ratio': rd(b + 0x140),
        'life_support': rw(b + 0x148),
        'has_impulse': rw(b + 0x14A) != 0,
        'impulse_repair': rd(b + 0x14C),
        'has_tractor': rw(b + 0x154) != 0,
        'tractor_strength': rw(b + 0x156),
        'tractor_repair': rd(b + 0x158),
        'tractor_energy': rd(b + 0x160),
        # +168/+16A/+16C: torps carried, probes carried, and a word always equal
        # to torps carried (destruct_energy per shipdesign.h field order).
        'destruct_energy': rw(b + 0x168),
        'probes_carried': rw(b + 0x16A),
        'torps_carried': rw(b + 0x16C),
        'crew_names': [s for s in (cstr(farptr(b + 0x172 + 4 * k)) for k in range(25)) if s],
    }

ships = [decode_ship(b) for b in bases]

# ---------- nations ----------
# struct: {name, adjective, command} ptrs, 6 doubles (aggression, bravery,
# loyalty, fanaticism, boarding_level, deviation), 2 null ptrs, intro ptr,
# 7 rating-message ptrs. The 6 bridge-officer names sit just BEFORE each
# nation struct (verified: Spock/Sulu/... before Federation, Martok/Kurn/
# Worf/... before Klingon).
# The Orion struct is truncated in the binary (the torpedo design array
# begins right after it) and carries no endgame messages; supply
# pirate-flavored stand-ins so every nation has a full table.
ORION_FALLBACK = {
    'endgame_intro': 'The Orion Pirate Guild has tallied the take. ',
    'endgame': [
        "Legendary!  Every cutthroat in the quadrant knows your name.",
        "A fine haul.  The Guild drinks to you tonight.",
        "Profitable enough.  You keep your ship.",
        "You broke even.  Barely.",
        "That cargo won't pay for the repairs.",
        "The Guild is repossessing your ship.",
        "Pathetic.  Even the Ferengi are laughing.",
    ],
}

nation_ptrs = sorted(set(s['_nation_ptr'] for s in ships))
nations = []
for b in nation_ptrs:
    nations.append({
        'name': cstr(farptr(b)),
        'adjective': cstr(farptr(b + 4)),
        'command': cstr(farptr(b + 8)),
        'aggression': rd(b + 0x0C),
        'bravery': rd(b + 0x14),
        'loyalty': rd(b + 0x1C),
        'fanaticism': rd(b + 0x24),
        'boarding_level': rd(b + 0x2C),
        'deviation': rd(b + 0x34),
        'endgame_intro': cstr(farptr(b + 0x44)),
        'endgame': [cstr(farptr(b + 0x48 + 4 * k)) for k in range(7)],
        'officers': [cstr(farptr(b - 0x18 + 4 * k)) for k in range(6)],
    })
    if not nations[-1]['endgame_intro'] or not all(nations[-1]['endgame']):
        nations[-1]['endgame_intro'] = ORION_FALLBACK['endgame_intro']
        nations[-1]['endgame'] = ORION_FALLBACK['endgame']
for s in ships:
    s['nation'] = nations[nation_ptrs.index(s['_nation_ptr'])]['adjective']
    del s['_nation_ptr']

# ---------- torpedo designs (contiguous array, stride 0x52) ----------
TORP_BASE, TORP_N, TORP_STRIDE = 0x2fb66, 7, 0x52
torps = []
for k in range(TORP_N):
    b = TORP_BASE + k * TORP_STRIDE
    torps.append({
        'name': cstr(farptr(b)),
        'desc': cstr(farptr(b + 4)),
        'velocity': rd(b + 0x0C),        # warp factor (mk7=30 fast, plasma=15)
        'damage': rd(b + 0x14),          # warhead (mk8=15 = mk7 10 +50% ✓)
        'arm_time': rd(b + 0x1C),
        'max_time_fuse': rd(b + 0x24),
        'max_prox': rd(b + 0x2C),
        'homing': rw(b + 0x34) != 0,     # plasma/Xplasma home
        'speed_variance': rd(b + 0x38),
        'warhead_type': rw(b + 0x40),    # 0 normal, 1 shield-bore, 2 plasma decay
        'charge_time': rd(b + 0x42),     # tube charge cycles (mk7=3 ✓ manual)
        'min_prox': rd(b + 0x4A),
    })

# ---------- probe designs (contiguous array, stride 0x3E) ----------
PROBE_BASE, PROBE_N, PROBE_STRIDE = 0x2fdf4, 7, 0x3E
probes = []
for k in range(PROBE_N):
    b = PROBE_BASE + k * PROBE_STRIDE
    probes.append({
        'name': cstr(farptr(b)),
        'desc': cstr(farptr(b + 4)),
        'velocity': rd(b + 0x0C),        # data probes 20+ ("warp 20" ✓)
        'damage': rd(b + 0x14),
        'arm_time': rd(b + 0x1C),
        'max_time_fuse': rd(b + 0x24),
        'max_prox': rd(b + 0x2C),
        'homing': rw(b + 0x34) != 0,
        'scan_range': rd(b + 0x36),      # 20000 for data probes
    })

# ---------- begin 1.65 mount counts (stats165) ----------
def stats165_overrides(path):
    d = open(path, 'rb').read()
    base165 = struct.unpack_from('<H', d, 8)[0] * 16
    def rw1(off):  return struct.unpack_from('<H', d, off)[0]
    def fp1(off):
        o, s = struct.unpack_from('<HH', d, off)
        if o == 0 and s == 0: return None
        return base165 + s * 16 + o
    def cs1(off, maxlen=60):
        if off is None or off >= len(d): return None
        e = d.find(b'\0', off, off + maxlen)
        if e < 0: return None
        try: return d[off:e].decode('ascii')
        except UnicodeDecodeError: return None
    # anchor: the far pointer to the "Heavy Cruiser" class-name string
    hc = d.find(b'Heavy Cruiser\0HC\0')
    assert hc >= 0, 'begin.exe: no Heavy Cruiser string'
    rec = None
    for i in range(0, len(d) - 4):
        o, s = struct.unpack_from('<HH', d, i)
        if s > 0x100 and base165 + s * 16 + o == hc:
            rec = i
            break
    assert rec is not None, 'begin.exe: no record pointing at Heavy Cruiser'
    STRIDE = 0x166
    recs = []
    b = rec
    while (n := cs1(fp1(b))) and n[0].isupper():
        recs.append(b); b -= STRIDE
    recs.reverse()
    b = rec + STRIDE
    while (n := cs1(fp1(b))) and n[0].isupper():
        recs.append(b); b += STRIDE
    out = {}
    for b in recs:
        nation = cs1(fp1(fp1(b + 8)))
        out[(nation, cs1(fp1(b)))] = {
            'banks': rw1(b + 0x64), 'tubes': rw1(b + 0x86), 'launchers': rw1(b + 0xA4),
        }
    return out

exe165 = os.path.join(os.path.dirname(exe) or '.', 'begin.exe')
if os.path.exists(exe165):
    by_name = {n['adjective']: n['name'] for n in nations}
    over = stats165_overrides(exe165)
    for s in ships:
        key = (by_name.get(s['nation']), s['name'])
        if key in over:
            o = over[key]
            if (s['banks'], s['tubes'], s['launchers']) != (o['banks'], o['tubes'], o['launchers']):
                print(f"stats165: {key[0]} {key[1]}: banks {s['banks']}->{o['banks']} "
                      f"tubes {s['tubes']}->{o['tubes']} launchers {s['launchers']}->{o['launchers']}")
            s.update(o)
else:
    print('begin.exe (v1.65) not found; keeping begin2 mount counts')

# ---------- AI message tables (spec §12.10) ----------
messages = {
    'destruct': [cstr(farptr(DSEG + 0xA618 + 4 * k)) for k in range(4)],
    'retreat': [cstr(farptr(DSEG + 0xA628 + 4 * k)) for k in range(8)],
    'insult': [cstr(farptr(DSEG + 0xA648 + 4 * k)) for k in range(5)],
}

os.makedirs(outdir, exist_ok=True)
for fname, obj in [('ships.json', ships), ('nations.json', nations),
                   ('torps.json', torps), ('probes.json', probes),
                   ('messages.json', messages)]:
    with open(os.path.join(outdir, fname), 'w') as f:
        json.dump(obj, f, indent=1)
    print(f"wrote {outdir}/{fname}")

#!/usr/bin/env python3
"""Adds recipes of prefabs to data/<game>/prefabs.json: the assemblies of the extracted levels, as
the editor finds them (scene/assemblies.h) and as "Select its assembly" selects them: a house of the
slums with its facades, roof and windows, a building of Haven, a tower of Spargus, a bridge with its
pillars. It drives a hidden editor through its remote control: each level opened, its assemblies
listed (list_assemblies), the ones too small, too thin (a pole, a pipe, a plank) or already met
(a house the level repeats, a near copy of it) left out, the largest of each level first, each named
after its place and what most of it is made of. The recipe names the level, an element of the assembly (its seed) and its
box (taken when an older extraction has no element of that name).

What was decided by looking at them is in tools/prefab_review.json, by game: "reject", the
assemblies not worth a prefab (level/seed); "names", level/seed -> the name given to it; "kinds",
level -> {kind found: what it is called there} (the farms' "trees" are orchard walls); "per_level",
level -> how many of its assemblies to look at (more in the levels rich in whole structures: the
slums, Haven, Spargus); "max_size", level -> the size its assemblies are cut at (whole blocks of the
slums and the industrial zone at 130 m; what is decided there keyed level@130/seed); "reviewed", the
levels looked at. The recipes written by hand
stay first and are kept; the ones this script made before ("auto") are made again.

Only names are written (levels, elements) and boxes: no data of the game. Run it from the
repository root after building the editor, with the games extracted, then rebuild (the data files
are copied next to the executable):

  python tools/gen_prefab_recipes.py [--game jak3|jak2] [--levels a,b] [--per-level 36] [--min-elements 10]
      [--min-size 12] [--min-thickness 4] [--min-triangles 300] [--max-size 60]
"""

import argparse
import hashlib
import json
import os
import re
import socket
import subprocess
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EDITOR = os.path.join(REPO, 'build', 'bin', 'open-goal-level-editor' + ('.exe' if os.name == 'nt' else ''))

# where a level is, by the beginning of its name (the first that matches)
PLACES = {
    'jak3': [('ctysl', 'Slums'), ('slumbset', 'Slums'), ('ctyind', 'Industrial zone'), ('ctyport', 'Port'),
             ('ctyfarm', 'Farms'), ('cty', 'Haven'), ('lcity', 'Haven'), ('lcty', 'Haven'), ('lfaccity', 'Haven'),
             ('lfaccty', 'Haven'), ('ltowcity', 'Haven'), ('rubbl', 'Ruined Haven'), ('rublcst', 'Ruined Haven'),
             ('comb', 'Catacombs'), ('des', 'Desert'), ('fac', 'Factory'), ('lfac', 'Factory'), ('forest', 'Forest'),
             ('freehq', 'Freedom HQ'), ('lfreeout', 'Freedom HQ'), ('gungame', 'Gun course'), ('halfpipe', 'Halfpipe'),
             ('hang', 'Hangar'), ('hiphog', 'Hip Hog'), ('intpalrf', 'Palace roof'), ('intpfall', 'Fallen palace'),
             ('ltower', 'Metal Head tower'), ('tower', 'Metal Head tower'), ('mhcity', 'Metal Head city'),
             ('mine', 'Mine'), ('nst', 'Nest'), ('onintent', "Onin's tent"), ('powergd', 'Power station'),
             ('precur', 'Precursor core'), ('rail', 'Precursor rail'), ('sew', 'Sewers'), ('stadium', 'Stadium'),
             ('temple', 'Temple'), ('vinroom', "Vin's room"), ('volcano', 'Volcano'), ('wasdoors', 'Spargus gate'),
             ('waspala', 'Spargus palace'), ('wasstad', 'Spargus arena'), ('was', 'Spargus')],
    'jak2': [('ctysl', 'Slums'), ('ctyind', 'Industrial zone'), ('ctyport', 'Port'), ('portwall', 'Port'),
             ('ctyfarm', 'Farms'), ('ctymark', 'Market'), ('ctypal', 'Palace'), ('cty', 'Haven'), ('lcity', 'Haven'),
             ('atoll', 'Pumping station'), ('cas', 'Castle'), ('consite', 'Construction site'), ('dig', 'Dig'),
             ('drill', 'Drill platform'), ('fordump', 'Fortress'), ('forexit', 'Fortress'), ('forresc', 'Fortress'),
             ('forest', 'Haven forest'), ('garage', 'Garage'), ('gungame', 'Gun course'), ('halfpipe', 'Halfpipe'),
             ('hideout', 'Hideout'), ('hiphog', 'Hip Hog'), ('kiosk', 'Kiosk'), ('lerl', 'Haven'),
             ('ltent', "Onin's tent"), ('onintent', "Onin's tent"), ('mincan', 'Mountain'), ('mountain', 'Mountain temple'),
             ('mtn', 'Mountain temple'), ('nest', 'Nest'), ('oracle', 'Oracle'), ('pal', 'Palace'), ('throne', 'Palace'),
             ('prison', 'Prison'), ('ruins', 'Dead Town'), ('sagehut', "Sage's hut"), ('sew', 'Sewers'),
             ('skate', 'Skate park'), ('stadium', 'Stadium'), ('strip', 'Strip mine'), ('tomb', "Mar's tomb"),
             ('under', 'Underport'), ('village', 'Sandover'), ('vinroom', "Vin's room")],
}

# what an assembly is, from the words of its parts' names (the most elements win), and its category
KINDS = [
    ('bridge', 'bridge', ['bridge']),
    ('tower', 'tower', ['tower', 'minaret', 'spire', 'chimney', 'mast', 'antenna', 'silo', 'obelisk']),
    ('stairs', 'stairs', ['stair', 'steps', 'ladder', 'ramp']),
    ('gate', 'gate', ['door', 'gate', 'arch', 'portal', 'entrance', 'doorway']),
    ('house', 'building', ['facade', 'roof', 'window', 'house', 'hut', 'shack', 'tent', 'building', 'bldg', 'balcony',
                           'awning', 'chimney', 'shutter', 'porch']),
    ('wall', 'wall', ['wall', 'fence', 'rail', 'barrier', 'barricade', 'pillar', 'column', 'post', 'seawall']),
    ('machine', 'generator', ['generator', 'engine', 'turbine', 'pump', 'machine', 'gear', 'crane', 'drill', 'motor',
                              'reactor', 'press', 'piston', 'conveyor', 'boiler', 'furnace', 'cog', 'tesla', 'coil']),
    ('vent', 'vent', ['vent', 'fan', 'exhaust', 'grate']),
    ('pipes', 'industrial', ['pipe', 'tank', 'valve', 'tube', 'duct', 'container', 'scaffold', 'girder', 'truss']),
    ('platform', 'platform', ['platform', 'plat', 'pad', 'ledge', 'deck', 'dock', 'pier', 'walkway', 'catwalk', 'floor']),
    ('rocks', 'nature', ['rock', 'boulder', 'cliff', 'stone', 'crystal', 'stalag', 'cave']),
    ('trees', 'nature', ['tree', 'bush', 'plant', 'cactus', 'palm', 'trunk', 'shroom', 'mushroom', 'root', 'log',
                         'branch', 'fern', 'flower']),
    ('statue', 'statue', ['statue', 'totem', 'idol', 'head', 'skull', 'monument', 'bust', 'carving', 'figure', 'mar']),
    ('stall', 'props', ['lamp', 'light', 'sign', 'banner', 'flag', 'stall', 'cart', 'table', 'bench', 'shelf',
                        'canopy', 'umbrella', 'pot', 'urn', 'vase', 'basket', 'barrel', 'crate', 'box']),
]
CATEGORY = {kind: cat for kind, cat, _ in KINDS}
CATEGORY['structure'] = 'building'
CATEGORY['building'] = 'building'


class Editor:
    def __init__(self, port, exe=EDITOR):
        self.proc = subprocess.Popen([exe, '--hidden', '--lang', 'en', '--remote-port', str(port)],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(300):
            try:
                self.sock = socket.create_connection(('127.0.0.1', port), timeout=1)
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit('the editor did not start')
        self.sock.settimeout(900)
        self.f = self.sock.makefile('rwb')
        self.n = 0

    def call(self, tool, args=None, check=True):
        self.n += 1
        self.f.write((json.dumps({'id': self.n, 'tool': tool, 'args': args or {}}) + '\n').encode())
        self.f.flush()
        while True:
            a = json.loads(self.f.readline())
            if a.get('id') == self.n:
                break
        if not a.get('ok'):
            if check:
                raise SystemExit('%s: %s' % (tool, a.get('error')))
            return {'error': a.get('error')}
        return a.get('result')

    def close(self):
        self.sock.close()
        self.proc.terminate()
        self.proc.wait(timeout=30)


def place_of(game, level):
    for prefix, place in PLACES[game]:
        if level.startswith(prefix):
            return place
    return level


def stem_words(stem):
    return [w for w in re.split(r'[-_.]', stem.lower()) if w and not re.fullmatch(r'\d+[a-z]?|[a-z]|mb', w)]


def kind_of(parts, size):
    """parts: [[stem, count, volume]...] (the most first); size: [x, y, z] in meters. The kind whose
    parts take the most room (a few facades over many small bolts)."""
    score = {}
    total = sum(p[2] for p in parts) or 1.0
    for stem, count, volume in parts:
        words = stem_words(stem)
        for kind, _, keys in KINDS:
            if any(k in w for w in words for k in keys):
                score[kind] = score.get(kind, 0) + volume
                break
    kind = max(score, key=score.get) if score else 'structure'
    if score and score[kind] < 0.15 * total:
        kind = 'structure'  # most of it named after nothing known
    # facades, roofs and windows make a house whatever hangs on them (pipes, beams, lamps)
    if score.get('house', 0) >= 0.25 * total:
        kind = 'house'
    width = max(size[0], size[2])
    if kind in ('house', 'wall', 'structure') and size[1] > 1.6 * width and size[1] > 15:
        kind = 'tower'
    elif kind == 'house' and (size[1] > 25 or width > 40):
        kind = 'building'
    return kind


def alike(parts_a, size_a, parts_b, size_b):
    """Two assemblies nearly the same: their parts (the 6 most frequent, with how many) 90 % in
    common and their size within 10 % on each axis, however turned."""
    if any(abs(x - y) > 0.1 * max(x, y, 1.0) for x, y in zip(sorted(size_a), sorted(size_b))):
        return False
    common = sum(min(n, parts_b.get(s, 0)) for s, n in parts_a.items())
    return common >= 0.9 * max(sum(parts_a.values()), sum(parts_b.values()))


def readable(stem):
    words = stem_words(stem)
    return ' '.join(words) or stem


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--game', default='jak3')
    ap.add_argument('--levels', default='', help='only these levels (a,b,c); the others keep their recipes')
    ap.add_argument('--min-elements', type=int, default=10)
    ap.add_argument('--min-size', type=float, default=12.0, help='meters across, at least')
    ap.add_argument('--per-level', type=int, default=36, help='at most this many per level, the largest first')
    ap.add_argument('--min-thickness', type=float, default=4.0,
                    help='meters, its box at least so on two sides: not a pole, a pipe, a plank')
    ap.add_argument('--min-triangles', type=int, default=300)
    ap.add_argument('--min-largest', type=float, default=3.0,
                    help='meters, its largest element at least: not a scatter of pebbles')
    ap.add_argument('--max-size', type=float, default=60.0, help='meters: a larger assembly is cut in parts')
    ap.add_argument('--port', type=int, default=47990)
    ap.add_argument('--editor', default=EDITOR, help='the editor to drive (a copy, to rebuild meanwhile)')
    args = ap.parse_args()
    game = args.game
    path = os.path.join(REPO, 'data', game, 'prefabs.json')
    with open(path, encoding='utf-8') as f:
        doc = json.load(f)
    review_path = os.path.join(REPO, 'tools', 'prefab_review.json')
    review = {}
    if os.path.exists(review_path):
        with open(review_path, encoding='utf-8') as f:
            review = json.load(f).get(game, {})
    reject = set(review.get('reject', []))
    given = review.get('names', {})
    kinds = review.get('kinds', {})  # level -> {kind: what it is called there}
    per_level = review.get('per_level', {})  # level -> how many to look at there (the rich ones: more)
    # level -> the size its assemblies are cut at (a city's whole blocks: larger than the default);
    # what was decided at another size is keyed level@size/seed
    sizes = review.get('max_size', {})
    only = set(l for l in args.levels.split(',') if l)
    kept = [r for r in doc['prefabs'] if not r.get('auto')]
    old_auto = [r for r in doc['prefabs'] if r.get('auto') and only and r['level'] not in only]
    # the boxes of the recipes written by hand: an assembly in one of them is that one already
    hand_boxes = [(r['level'], r['box']) for r in kept if r.get('box') and not r.get('assembly')]
    names = {r['name'].lower() for r in kept + old_auto}
    seen = set()
    accepted = {}  # the main part -> [(parts, size)] of the assemblies taken: their near copies are left out
    for r in old_auto:
        if r.get('signature'):
            seen.add(r['signature'])
    counter = {}
    made = []
    e = Editor(args.port, args.editor)
    try:
        sel = {'game': 'jak2'} if game == 'jak2' else {}
        for _ in range(300):
            r = e.call('list_levels', sel, check=False)
            if 'error' not in r:
                break
            time.sleep(1)
        levels = [l['name'] for l in r['levels']]
        for level in levels:
            if only and level not in only:
                continue
            res = e.call('open_level', {'name': level, 'game': 'jak2' if game == 'jak2' else '', 'actors': False,
                                        'discard_changes': True}, check=False)
            if 'error' in res:
                print('  %s: %s' % (level, res['error']))
                continue
            cut = sizes.get(level, args.max_size)
            key = level if cut == args.max_size else '%s@%g' % (level, cut)
            found = e.call('list_assemblies', {'min_elements': args.min_elements, 'min_size': args.min_size,
                                                'max_size': cut, 'limit': 100000})['assemblies']
            place = place_of(game, level)
            count = 0
            for a in found:
                seed = a.get('seed')
                if not seed:
                    continue
                lo, hi = a['box']
                size = [hi[k] - lo[k] for k in range(3)]
                if (sorted(size)[1] < args.min_thickness or a.get('triangles', 0) < args.min_triangles or
                        a.get('largest', 1e9) < args.min_largest):
                    continue
                mid = [(lo[k] + hi[k]) / 2 for k in range(3)]
                if any(l == level and all(b[0][k] <= mid[k] <= b[1][k] for k in range(3)) for l, b in hand_boxes):
                    continue
                # the same assembly again (a house the level repeats): once
                signature = hashlib.sha1(('%s|%s' % (','.join('%s*%d' % (p[0], p[1]) for p in sorted(a['parts'])),
                                                     ','.join(str(round(v)) for v in sorted(size)))).encode()).hexdigest()[:12]
                if signature in seen:
                    continue
                seen.add(signature)
                # nearly the same parts (90 %) and size (10 %) as one taken: a variant of it
                parts = {p[0]: p[1] for p in a['parts']}
                same = accepted.setdefault(a['parts'][0][0], [])
                if any(alike(parts, size, p2, s2) for p2, s2 in same):
                    continue
                same.append((parts, size))
                # the first ones of the level, those looked at (tools/prefab_review.json): a rejected
                # one still counts, so that the next ones are not new to the review
                count += 1
                if count > per_level.get(level, args.per_level):
                    break
                if '%s/%s' % (key, seed) in reject:
                    continue
                kind = kind_of(a['parts'], size)
                category = CATEGORY.get(kind, 'building')
                kind = kinds.get(level, {}).get(kind, kind)
                name = given.get('%s/%s' % (key, seed))
                if name:
                    base, k = name, 2
                    while name.lower() in names:
                        name = '%s %d' % (base, k)
                        k += 1
                else:
                    counter[(place, kind)] = counter.get((place, kind), 0) + 1
                    name = '%s %s %d' % (place, kind, counter[(place, kind)])
                    while name.lower() in names:
                        counter[(place, kind)] += 1
                        name = '%s %s %d' % (place, kind, counter[(place, kind)])
                names.add(name.lower())
                main_parts = ', '.join(readable(p[0]) for p in sorted(a['parts'], key=lambda p: -p[2])[:3])
                made.append({'name': name, 'category': category, 'level': level,
                             'assembly': seed, 'max_size': cut,
                             'box': [[round(v, 1) for v in lo], [round(v, 1) for v in hi]], 'auto': True,
                             'signature': signature,
                             'description': '%s of %s (%s): %d elements, %.0f x %.0f x %.0f m (%s...).' %
                                            (kind.capitalize(), place, level, a['elements'], size[0], size[1],
                                             size[2], main_parts)})
            print('%s: %d of %d' % (level, min(count, per_level.get(level, args.per_level)), len(found)), flush=True)
    finally:
        e.close()
    # as the file is written by hand: a recipe per line
    recipes = kept + old_auto + made
    lines = ['{'] + [' %s: %s,' % (json.dumps(k), json.dumps(v, ensure_ascii=False)) for k, v in doc.items() if k != 'prefabs']
    lines.append(' "prefabs": [')
    lines += ['  %s%s' % (json.dumps(r, ensure_ascii=False), ',' if i + 1 < len(recipes) else '') for i, r in enumerate(recipes)]
    lines += [' ]', '}']
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(lines) + '\n')
    print('%d recipes written by hand, %d found' % (len(kept), len(old_auto) + len(made)))


if __name__ == '__main__':
    main()

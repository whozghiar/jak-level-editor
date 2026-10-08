#!/usr/bin/env python3
"""Builds "Ironworks", a district of Haven City in the style of Jak 3's industrial zone (ctyinda,
ctyindb), about as large (448 x 448 m), through the editor's MCP server: the tools an assistant
gets, called by a small MCP client.

The district is made of the zone's own modules, placed as the game places them:

- 15 closed blocks, each a ring of four "facade columns": a ground facade, the upper facade on it
  and everything the game hangs on them (pipes, lamps, doors, windows, stains, red lights and
  their glow), taken from the zone's facades of the same kind; flat roofs, and the game's corner
  joints on the orange blocks;
- the ends of the streets closed by two wall buildings back to back, roofed;
- catwalks along two streets with a T junction, and the zone's ramp down into a yard;
- the yard: floor vents with their red glow, bent vent pipes (from Haven's ctygenb), warning
  lights, a pipe bridge on pillars, crates; big pipes over the streets; neon signs of the slums;
- the ground, and the same ground facing down under it.

It then generates the nav meshes of the streets, the yard and the catwalks (the ground connected to
a few street points: not the roofs, ledges or the insides of the blocks) and saves the project.

Where each piece goes is read from the extracted levels of the library (the matrices of the zone's
instances), so it needs a Jak 3 library extracted with the editor. It holds no data of the game:
only names, indices and positions.

Usage (from the repository root, after building the editor):

  python tools/build_industrial_district.py [--out build/demo/ironworks.glb] [--views <folder>]
                                            [--nav <file.json>] [--library library/jak3]
                                            [--port 47891]

It starts a hidden editor on `--port` (the preferences are left unchanged) and its MCP server
(`--mcp`), builds the district, generates its nav meshes and saves the project to `--out`. With
`--views`, it saves two pictures of the district from capture_view (district-aerial.png,
district-yard.png); with `--nav`, the nav meshes for the game (export_navmesh).
"""

import argparse
import base64
import json
import math
import os
import random
import socket
import struct
import subprocess
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EDITOR = os.path.join(REPO, 'build', 'bin', 'open-goal-level-editor' + ('.exe' if os.name == 'nt' else ''))


# ------------------------------------------------------------------------------------------------
# matrices (4x4, rows)

def mmul(a, b):
    return [[sum(a[r][i] * b[i][c] for i in range(4)) for c in range(4)] for r in range(4)]


def tr(x, y, z):
    return [[1, 0, 0, x], [0, 1, 0, y], [0, 0, 1, z], [0, 0, 0, 1]]


def ry(deg):
    a = math.radians(deg)
    c, s = math.cos(a), math.sin(a)
    return [[c, 0, s, 0], [0, 1, 0, 0], [-s, 0, c, 0], [0, 0, 0, 1]]


def rx(deg):
    a = math.radians(deg)
    c, s = math.cos(a), math.sin(a)
    return [[1, 0, 0, 0], [0, c, -s, 0], [0, s, c, 0], [0, 0, 0, 1]]


def scl(x, y, z):
    return [[x, 0, 0, 0], [0, y, 0, 0], [0, 0, z, 0], [0, 0, 0, 1]]


def at(x, y, z, yaw=0.0, s=1.0):
    return mmul(mmul(tr(x, y, z), ry(yaw)), scl(s, s, s))


def axis_scales(m):
    return [math.sqrt(sum(m[r][k] ** 2 for r in range(3))) for k in range(3)]


def frame_of(m):
    """The rotation and position of a matrix, without its scale."""
    s = axis_scales(m)
    return [[m[r][k] / (s[k] if k < 3 else 1) for k in range(4)] for r in range(3)] + [[0, 0, 0, 1]]


def inverse_frame(m):
    rot = [[m[c][r] for c in range(3)] for r in range(3)]
    t = [-sum(rot[r][k] * m[k][3] for k in range(3)) for r in range(3)]
    return [rot[0] + [t[0]], rot[1] + [t[1]], rot[2] + [t[2]], [0, 0, 0, 1]]


def yaw_of(m):
    return math.degrees(math.atan2(m[0][2], m[0][0]))


def euler_deg(r):
    """A rotation matrix as the editor's rotation_deg (x, y, z: r = rz * ry * rx)."""
    y = math.asin(max(-1.0, min(1.0, -r[2][0])))
    if abs(r[2][0]) < 1 - 1e-6:
        x, z = math.atan2(r[2][1], r[2][2]), math.atan2(r[1][0], r[0][0])
    else:
        # turned a quarter around y (a ramp along x): x and z turn about the same axis, all on x
        x, z = math.atan2(-r[2][0] * r[0][1], r[1][1]), 0.0
    return [math.degrees(x), math.degrees(y), math.degrees(z)]


# ------------------------------------------------------------------------------------------------
# the zone, read from the library: each prototype's bounds, each instance's matrix, the effects

class Zone:
    def __init__(self, library, level):
        self.level = level
        path = os.path.join(library, level, 'decor', f'{level}-background.glb')
        with open(path, 'rb') as f:
            data = f.read()
        n = struct.unpack_from('<I', data, 12)[0]
        self.gltf = json.loads(data[20:20 + n])
        length = struct.unpack_from('<I', data, 20 + n)[0]
        self.bin = data[28 + n:28 + n + length]
        self.protos, self.inst = {}, []
        nodes = self.gltf['nodes']
        parent = {c: i for i, nd in enumerate(nodes) for c in nd.get('children', [])}
        for i, nd in enumerate(nodes):
            p = parent.get(i)
            if p is None or not any(k in nodes[p].get('name', '') for k in ('-tie-', '-shrub-')):
                continue
            kids = [c for c in nd.get('children', []) if 'mesh' in nodes[c]] or ([i] if 'mesh' in nd else [])
            if not kids:
                continue
            self.protos[nd['name']] = self.bounds(nodes[kids[0]]['mesh'])
            self.inst += [(nd['name'], self.matrix(nodes[c])) for c in kids]
        # the particle effects (actors drawn without a model), as 'fx:<name>'
        for nd in nodes:
            ex = nd.get('extras', {})
            lump = ex.get('lump')
            if ex.get('etype') == 'part-spawner' and isinstance(lump, dict):
                art = str(lump.get('art-name', ''))
                if art.startswith('group-'):
                    self.inst.append(('fx:' + art[len('group-'):], self.matrix(nd)))
        del self.bin

    @staticmethod
    def matrix(nd):
        if 'matrix' in nd:
            m = nd['matrix']
            return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
        t, q, s = nd.get('translation', [0, 0, 0]), nd.get('rotation', [0, 0, 0, 1]), nd.get('scale', [1, 1, 1])
        x, y, z, w = q
        r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
             [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
             [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
        return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)] + [[0, 0, 0, 1]]

    def bounds(self, mesh):
        lo, hi = [1e30] * 3, [-1e30] * 3
        for prim in self.gltf['meshes'][mesh]['primitives']:
            acc = self.gltf['accessors'][prim['attributes']['POSITION']]
            view = self.gltf['bufferViews'][acc['bufferView']]
            start = view.get('byteOffset', 0) + acc.get('byteOffset', 0)
            stride = view.get('byteStride', 0) or 12
            for i in range(acc['count']):
                v = struct.unpack_from('<3f', self.bin, start + stride * i)
                for k in range(3):
                    lo[k], hi[k] = min(lo[k], v[k]), max(hi[k], v[k])
        return lo, hi

    def item(self, name, m):
        """A place_many item putting `name` at the world matrix m. A decor part's origin in the
        editor is the bottom center of its geometry; an effect's is its actor's position."""
        s = axis_scales(m)
        r = [[m[i][k] / s[k] for k in range(3)] for i in range(3)]
        if name.startswith('fx:'):
            it = {'kind': 'object', 'name': name[3:], 'position': [round(m[i][3], 4) for i in range(3)]}
        else:
            lo, hi = self.protos[name]
            c = [(lo[0] + hi[0]) / 2, lo[1], (lo[2] + hi[2]) / 2]
            it = {'kind': 'decor', 'name': name, 'level': self.level,
                  'position': [round(m[i][3] + sum(m[i][k] * c[k] for k in range(3)), 4) for i in range(3)]}
            if any(abs(v - 1) > 1e-4 for v in s):
                it['scale'] = round(s[0], 5) if max(s) - min(s) < 1e-4 else [round(v, 5) for v in s]
        if abs(r[1][1] - 1) < 1e-4:
            it['yaw_deg'] = round(math.degrees(math.atan2(r[0][2], r[0][0])), 4)
        else:
            it['rotation_deg'] = [round(v, 4) for v in euler_deg(r)]
        return it


# ------------------------------------------------------------------------------------------------
# modules of the zone

# what does not belong to a facade column: the other facades, the ground and its craters, the
# catwalks and the steaming vents on them, the floor vents
NOT_IN_COLUMNS = ('facade-BOT', 'facade-TOP', 'ground-64x64', 'ground-seamcover', 'ground-crater', 'catwalk',
                  'ground-panels', 'ground-bigvent', 'ground-vent-small', 'rising-steam', 'floor-vent')


class Column:
    """A ground facade of the zone, the upper facade on it and what is around them, in the frame
    of the ground facade."""

    def __init__(self, zone, index):
        facades = [(i, n, m) for i, (n, m) in enumerate(zone.inst) if 'facade-BOT-' in n]
        i0, self.bot, m0 = facades[index]
        self.zone = zone
        lo, hi = zone.protos[self.bot]
        inv = inverse_frame(frame_of(m0))
        self.members = [(self.bot, mmul(inv, m0))]
        for i, (name, m) in enumerate(zone.inst):
            if i == i0:
                continue
            p = [sum(inv[r][k] * m[k][3] for k in range(3)) + inv[r][3] for r in range(3)]
            if 'facade-TOP-' in name:
                # the upper facade standing on it, turned the same way
                if abs(p[0]) < 6 and abs(p[2]) < 6 and 8 < p[1] < 32 and abs(yaw_of(m) - yaw_of(m0)) % 360 < 1:
                    self.members.insert(1, (name, mmul(inv, m)))
                continue
            if any(k in name for k in NOT_IN_COLUMNS):
                continue
            if -13 <= p[0] <= 9 and lo[2] - 1 <= p[2] <= hi[2] + 1 and -1 <= p[1] <= 62:
                self.members.append((name, mmul(inv, m)))
        # along the facade: half its length, and the shift that centers it
        self.stretched = any(abs(v - 1) > 1e-3 for v in axis_scales(m0))
        self.half = (hi[2] - lo[2]) / 2
        self.shift = -(lo[2] + hi[2]) / 2

    def items(self, m):
        return [self.zone.item(name, mmul(m, rel)) for name, rel in self.members]


class Assembly:
    """Pieces of the zone around one of its instances (`pick`, the index-th of its name), in the
    frame of that instance."""

    def __init__(self, zone, pick, near, radius, index=0, where=None):
        self.zone = zone
        if where:
            self.members = [(n, m) for n, m in zone.inst if where(n, m)]
            return
        n0, m0 = [(n, m) for n, m in zone.inst if n == pick][index]
        inv = inverse_frame(frame_of(m0))
        self.members = []
        for n, m in zone.inst:
            if (n == pick and m is not m0) or (n != pick and not any(k in n for k in near)):
                continue
            if math.dist([m[r][3] for r in range(3)], [m0[r][3] for r in range(3)]) <= radius:
                self.members.append((n, mmul(inv, m)))

    def items(self, m):
        return [self.zone.item(n, mmul(m, rel)) for n, rel in self.members]


# ------------------------------------------------------------------------------------------------
# the district

G = 7.99            # the ground
CELLS = [-180, -60, 60, 180]   # the blocks, 120 m apart: the streets at -120, 0 and 120
EDGE = 224          # 448 x 448 m
YARD = (60.0, -60.0)
GROUND = 'city-ind-ground-64x64.mb'

# i (x), j (z): A grey (80w-01 facades), B orange with towers (80w-03), D mansard roofs (BOT-80w-02
# under TOP-80w-04), None the yard
LAYOUT = {
    (0, 0): 'A', (1, 0): 'B', (2, 0): 'D', (3, 0): 'A',
    (0, 1): 'D', (1, 1): 'A', (2, 1): None, (3, 1): 'B',
    (0, 2): 'B', (1, 2): 'D', (2, 2): 'A', (3, 2): 'D',
    (0, 3): 'A', (1, 3): 'B', (2, 3): 'D', (3, 3): 'B',
}
# the facade columns of each kind (level, index of the ground facade): zone facades of that kind
# at their own length (the game stretches some along the street to fit a place; four of those
# would not meet at the corners). The zone has two mansard columns at their own length: each
# block of them shows both twice
COLUMNS = {
    'A': [('ctyinda', 0), ('ctyinda', 1), ('ctyinda', 2), ('ctyinda', 4), ('ctyindb', 16), ('ctyindb', 17)],
    'B': [('ctyinda', 39), ('ctyindb', 13), ('ctyindb', 14), ('ctyindb', 15)],
    'D': [('ctyindb', 20), ('ctyindb', 22)] * 2,
}
# the 60 m wall buildings closing the streets (BOT-60w-01 under TOP-60w-04)
WALLS = [('ctyinda', 27), ('ctyinda', 29), ('ctyinda', 32), ('ctyinda', 26), ('ctyinda', 28), ('ctyindb', 24),
         ('ctyindb', 25), ('ctyindb', 26), ('ctyindb', 27), ('ctyindb', 28), ('ctyindb', 30)]
# the roofs: their height, just under the top of the walls, measured on each kind of block
ROOF_Y = {'A': 50.6, 'B': 49.0, 'D': 50.4}


class District:
    def __init__(self, library):
        self.zones = {lv: Zone(library, lv) for lv in ('ctyinda', 'ctyindb')}
        self.a = self.zones['ctyinda']
        self.b = self.zones['ctyindb']
        self.columns = {}
        # a warning light: its base (the 4th of the zone stands upright), its bulb, the red glow
        self.redlight = Assembly(self.a, 'city-ind-redlight-base-01.mb', ('redlight-bulb', 'fx:ctyinda-bulb-red-glow'),
                                 1.5, 3)
        # a floor vent and its red glow
        self.vent = Assembly(self.b, 'city-ind-ground-bigvent2.mb', ('bigvent-bars', 'fx:ctyindb-red-floor-vent'), 9)
        # the zone has no stacks of its own: a bent vent pipe of Haven (ctygenb), its vents, lights and
        # glow, lifted so that its foot stands where it is put
        genb = Zone(library, 'ctygenb')
        self.stack = Assembly(genb, 'city-bent-vent-01.mb', ('bent-vent', 'fx:ctygenb-blue-vents-pipe-bent'), 5)
        self.stack_foot = tr(0, -genb.protos['city-bent-vent-01.mb'][0][1], 0)
        # neon signs of the slums: the sign and the effect that draws it
        self.signs = [Assembly(Zone(library, level), sign, (fx,), 1, index) for level, sign, fx, index in [
            ('ctyslumb', 'city-ind-stickout-sign-01.mb', 'fx:sign-ctyslumb-tall-a', 3),
            ('ctyslumc', 'city-ind-stickout-sign-01.mb', 'fx:sign-ctyslumc-tall-b', 2),
            ('ctysluma', 'city-slum-stickout-sign-01.mb', 'fx:ctysluma-sign-crimson-4x8', 8)]]
        # the ramp of a catwalk of ctyinda, from its foot to its joint with the flat catwalk (at x 1071.6)
        self.ramp = Assembly(self.a, None, None, 0, where=lambda n, m: 'catwalk' in n and 1010 < m[0][3] < 1070 and
                             abs(m[2][3] - 1144.3) < 1 and abs(m[1][3] - 23.84) >= 0.05)

    def column(self, level, index):
        if (level, index) not in self.columns:
            self.columns[(level, index)] = Column(self.zones[level], index)
        return self.columns[(level, index)]

    def part(self, name, x, y, z, yaw=0.0, s=1.0, zone=None):
        """A part with the game's origin of its prototype at x, y, z."""
        return (zone or self.a).item(name, at(x, y, z, yaw, s))

    @staticmethod
    def flat(name, x, y, z, yaw, s):
        """A part with the bottom center of its geometry at x, y, z."""
        return {'kind': 'decor', 'name': name, 'level': 'ctyinda', 'position': [x, y, z], 'yaw_deg': yaw, 'scale': s}

    def ground(self):
        items = []
        for ix in range(-3, 4):
            for iz in range(-3, 4):
                yaw = 90 if (ix * 7 + iz * 3) % 2 else 0
                items.append(self.part(GROUND, 64 * ix, G, 64 * iz, yaw, 1.001))
                # underneath, facing down: the district is closed from below too
                items.append({'kind': 'decor', 'name': GROUND, 'level': 'ctyinda', 'position': [64 * ix, G - 0.08, 64 * iz],
                              'rotation_deg': [180, yaw, 0], 'scale': 1.001})
        for k in range(-3, 3):
            for t in range(-3, 4):
                items.append(self.part('city-ind-ground-seamcover-64.mb', 32 + 64 * k, G, 64 * t, 90))
                items.append(self.part('city-ind-ground-seamcover-64.mb', 64 * t, G, 32 + 64 * k, 0))
        return items

    def block(self, cx, cz, kind, seed):
        """Four columns facing out, their chamfered ends meeting at the corners."""
        items = []
        for k, src in enumerate(random.Random(seed).sample(COLUMNS[kind], 4)):
            c = self.column(*src)
            assert not c.stretched, src
            items += c.items(mmul(mmul(tr(cx, 8.01, cz), ry(-90 * k)), tr(c.half, 0, c.shift)))
        return items

    def roof(self, cx, cz, kind, seed):
        y = ROOF_Y[kind]
        if kind == 'D':
            items = [self.flat(GROUND, cx, y, cz, 0, [68.8 / 64, 1, 68.8 / 63.97])]
        else:
            # two crossed bands of floor, into the walls on every side and at the cut corners
            half, band = (39.8, 33.5) if kind == 'A' else (41.0, 36.0)
            items = [self.flat(GROUND, cx, y, cz, 0, [2 * half / 64, 1, 2 * band / 63.97]),
                     self.flat(GROUND, cx, y + 0.03, cz, 0, [2 * band / 64, 1, 2 * half / 63.97])]
        if kind == 'B':
            # the game's corner joints of these facades, at the end of each one, turned 135 degrees
            lo, hi = self.a.protos['city-ind-facade-BOT-80w-25h-03.mb']
            for k in range(4):
                m = mmul(mmul(tr(cx, 8.01, cz), ry(-90 * k)), tr((hi[2] - lo[2]) / 2, 0, -(lo[2] + hi[2]) / 2))
                items.append(self.a.item('city-ind-joint-TOP-03.mb', mmul(m, mmul(tr(-1.29, 30.24, 43.31), ry(135)))))
                items.append(self.a.item('city-ind-joint-BOT-03.mb', mmul(m, mmul(tr(-2.67, 8.48, 44.66), ry(135)))))
        # warning lights on the corners, steam stacks or a floor vent
        corner = 30.0 if kind == 'A' else 31.0
        for sx in (-1, 1):
            for sz in (-1, 1):
                yaw = random.Random(seed + sx * 3 + sz).choice((0, 90, 180, -90))
                items += self.redlight.items(at(cx + sx * (corner - 3), y, cz + sz * (corner - 3), yaw, 2.2))
        rnd = random.Random(seed * 7 + 1)
        if rnd.random() < 0.6:
            for _ in range(rnd.choice((1, 2))):
                a = rnd.uniform(0, 2 * math.pi)
                items += self.stack.items(mmul(at(cx + 14 * math.cos(a), y, cz + 14 * math.sin(a),
                                                  rnd.choice((0, 90, 180, -90))), self.stack_foot))
        else:
            items += self.vent.items(at(cx + rnd.uniform(-6, 6), y + 0.1, cz + rnd.uniform(-6, 6), rnd.choice((0, 90))))
        return items

    def street_ends(self):
        """Two wall buildings back to back where each street meets the edge, and a roof over the
        trench between their leaning tops."""
        items = []
        n = 0
        for s in (-120, 0, 120):
            for side in 'SNWE':
                inward = {'S': -90, 'N': 90, 'W': 0, 'E': 180}[side]
                for which in (0, 1):
                    c = self.column(*WALLS[(2 * n + which) % len(WALLS)])
                    lo, hi = c.zone.protos[c.bot]
                    d = hi[0] if which == 0 else hi[0] - 2 * lo[0]   # from the edge
                    o = {'S': (s, -EDGE + d), 'N': (s, EDGE - d), 'W': (-EDGE + d, s), 'E': (EDGE - d, s)}[side]
                    yaw = inward + 180 if which == 0 else inward
                    items += c.items(mmul(mmul(tr(o[0], 8.01, o[1]), ry(yaw)), tr(0, 0, c.shift)))
                d = 11.83
                x, z, yaw = {'S': (s, -EDGE + d, 0), 'N': (s, EDGE - d, 0), 'W': (-EDGE + d, s, 90),
                             'E': (EDGE - d, s, 90)}[side]
                items.append(self.flat(GROUND, x, 38.6, z, yaw, [48 / 64, 1, 26 / 63.97]))
                n += 1
        return items

    def catwalks(self):
        """Along the street x = 0 and the street z = 120 (a T where they meet), a branch and the
        ramp down into the yard."""
        step, items = 15.95, []
        tiles = [(0.0, 120 - step * k, 'z', 'T90' if k == 0 else 'T0' if k == 11 else 'main') for k in range(21)]
        tiles += [(step * k, 120.0, 'x', 'main') for k in range(-13, 14) if k]
        tiles.append((step, 120 - step * 11, 'x', 'main'))
        for x, z, axis, kind in tiles:
            if kind == 'main':
                items.append(self.part('city-ind-catwalk-main-01.mb', x, 23.84, z, 0 if axis == 'z' else 90, 0.997))
            else:
                items.append(self.part('city-ind-catwalk-T-01.mb', x, 23.87, z, 90 if kind == 'T90' else 0))
        support = 'city-ind-catwalk-support-02.mb'
        items += [self.part(support, 0.0, 8.0, 120 - step * k, 0, 0.922) for k in range(2, 20, 3) if k != 11]
        items += [self.part(support, step * k, 8.0, 120.0, 90, 0.922) for k in list(range(-12, 0, 3)) + list(range(3, 13, 3))]
        # the ramp goes down towards -x in the zone: turned to go down towards +x, into the yard
        items += self.ramp.items(mmul(mmul(tr(step * 1.5, 0, 120 - step * 11), ry(180)), tr(-1071.6, 0, -1144.3)))
        return items

    def pipes(self):
        """Big pipes from one side of a street to the other."""
        items, s = [], 0.9
        length = 20.69 * s
        for st, along, where, y in [(-120, 'x', -150, 36), (-120, 'x', 40, 42), (120, 'x', -170, 34),
                                    (120, 'x', 20, 40), (0, 'x', 170, 38), (-120, 'z', -190, 38), (0, 'z', 75, 36),
                                    (120, 'z', -40, 44), (-120, 'z', 175, 34), (0, 'z', -170, 40)]:
            yaw = 0 if along == 'x' else 90
            for d, name in [(-0.5, 'straight'), (0.5, 'straight'), (-1, 'ring'), (0, 'ring'), (1, 'ring')]:
                p = st + d * length
                x, z = (p, where) if along == 'x' else (where, p)
                items.append(self.part(f'city-ind-bigpipe-{name}-01.mb', x, y, z, yaw, s))
        return items

    def yard(self):
        x0, z0 = YARD
        items = []
        for dx, dz, yaw in [(-12, -38, 0), (42, -38, 90), (-12, 38, 90), (42, 38, 0), (35, 45, 0), (-45, -50, 90)]:
            items += self.vent.items(at(x0 + dx, G + 0.02, z0 + dz, yaw))
        for dx, dz, yaw in [(66, -32, 180), (66, 28, 180), (8, -70, 90), (-60, 42, 0), (-40, 40, 0), (50, 50, 90),
                            (-20, -65, 180)]:
            items += self.stack.items(mmul(at(x0 + dx, 8.01, z0 + dz, yaw), self.stack_foot))
        for dx, dz in [(34, -14), (34, 14), (-8, -14), (-8, 14), (70, -60), (70, 70)]:
            items += self.redlight.items(at(x0 + dx, 8.01, z0 + dz, 0, 2.5))
        # a pipe bridge across the yard, a big pipe and a small one on cross beams and pillars
        xa, xb, zb = -24.0, 143.6, -100.0
        big, small = 20.69 * 0.9, 27.84 * 0.5
        n = int(math.ceil((xb - xa) / big))
        items += [self.part('city-ind-bigpipe-straight-01.mb', xa + big * (k + 0.5), 34.0, zb, 0, 0.9) for k in range(n)]
        items += [self.part('city-ind-bigpipe-ring-01.mb', xa + big * k, 34.0, zb, 0, 0.9) for k in range(n + 1)]
        for k in range(int(math.ceil((xb - xa) / small))):
            items.append(self.part('city-ind-smallpipe-straight-01.mb', xa + small * (k + 0.5), 33.14, zb - 4, 0, 0.5))
            items.append(self.part('city-ind-smallpipe-ring-01.mb', xa + small * k, 33.14, zb - 4, 0, 0.5))
        beam = 'city-ind-beam-metal-lowres-01.mb'
        for x in (22.0, 62.0, 104.0):
            for z in (zb + 2.5, zb - 5.0):
                items.append(self.a.item(beam, mmul(tr(x, 8.0 + 23.15 / 2, z), scl(1.2, 23.15 / 18.25, 1.2))))
            items.append(self.a.item(beam, mmul(mmul(tr(x, 31.75, zb - 1.25), rx(90)), scl(1.2, 8.7 / 18.25, 1.2))))
            items += self.redlight.items(at(x, 34.0 + 0.9 * 1.83, zb, 90, 2.0))
        # crates of the city, raised floor panels
        for x, z, yaw in [(125, -122, 0), (129, -70, 90), (26, -8, 30), (96, -131, 0), (129, 8, 90)]:
            items.append({'kind': 'model', 'name': 'ctycrate-metal', 'position': [x, 8.0, z], 'yaw_deg': yaw})
        for x, z in [(118, -128), (132, -60), (32, -2), (104, -134), (121, -112), (134, 16)]:
            items.append({'kind': 'model', 'name': 'ctycrate-blue', 'position': [x, 8.0, z], 'yaw_deg': (x * 37) % 360})
        items.append(self.part('city-ind-ground-panels-02.mb', 104, G, -40, 0, 0.456))
        items.append(self.part('city-ind-ground-panels-02.mb', 18, G, -128, 90, 0.456))
        return items

    def neon_signs(self):
        items = []
        for x, z, yaw, which in [(60, 15.2, 0, 0), (129.6, -60, 90, 1), (-15.2, -80, 90, 2), (-101.75, 190, 90, 0),
                                 (60, -128.2, 0, 1), (138, 104.8, 0, 2)]:
            items += self.signs[which].items(at(x, 45.0, z, yaw))
        return items

    def layers(self):
        out = [('ground', self.ground())]
        seed = 11
        for (i, j), kind in LAYOUT.items():
            if kind:
                seed += 1
                out.append((f'block {i},{j} ({kind})', self.block(CELLS[i], CELLS[j], kind, seed) +
                            self.roof(CELLS[i], CELLS[j], kind, seed)))
        out += [('ends of the streets', self.street_ends()), ('catwalks', self.catwalks()), ('pipes', self.pipes()),
                ('yard', self.yard()), ('neon signs', self.neon_signs())]
        return out


# ------------------------------------------------------------------------------------------------
# the editor, through its MCP server

class Mcp:
    """A minimal MCP client: JSON-RPC lines on the server's stdin and stdout."""

    def __init__(self, port):
        self.editor = subprocess.Popen([EDITOR, '--hidden', '--remote-port', str(port)],
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        # the MCP server starts an editor of its own when none answers: wait for this one first
        for _ in range(150):
            try:
                socket.create_connection(('127.0.0.1', port), timeout=1).close()
                break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit('the editor did not start')
        self.server = subprocess.Popen([EDITOR, '--mcp', '--remote-port', str(port)], stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.next_id = 1
        self.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                'clientInfo': {'name': 'build_industrial_district', 'version': '1'}})
        self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})

    def send(self, msg):
        self.server.stdin.write((json.dumps(msg) + '\n').encode())
        self.server.stdin.flush()

    def rpc(self, method, params):
        i = self.next_id
        self.next_id += 1
        self.send({'jsonrpc': '2.0', 'id': i, 'method': method, 'params': params})
        while True:
            line = self.server.stdout.readline()
            if not line:
                raise SystemExit('the MCP server stopped')
            answer = json.loads(line)
            if answer.get('id') == i:
                return answer

    def call(self, tool, args=None, image=None):
        result = self.rpc('tools/call', {'name': tool, 'arguments': args or {}})['result']
        text = next((c['text'] for c in result['content'] if c['type'] == 'text'), '')
        if result.get('isError'):
            raise SystemExit(f'{tool}: {text}')
        for c in result['content']:
            if c['type'] == 'image' and image:
                with open(image, 'wb') as f:
                    f.write(base64.b64decode(c['data']))
        return json.loads(text) if text.startswith('{') else text

    def close(self):
        self.server.stdin.close()
        self.server.wait(timeout=20)
        self.editor.terminate()
        try:
            self.editor.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.editor.kill()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--out', default=os.path.join(REPO, 'build', 'demo', 'ironworks.glb'))
    ap.add_argument('--views', help='a folder for district-aerial.png and district-yard.png')
    ap.add_argument('--nav', help='writes the nav meshes for the game to this .json file')
    ap.add_argument('--library', default=os.path.join(REPO, 'library', 'jak3'))
    ap.add_argument('--port', type=int, default=47891)
    args = ap.parse_args()

    t0 = time.time()
    district = District(args.library)
    layers = district.layers()
    print(f'read the industrial zone in {time.time() - t0:.1f} s')
    m = Mcp(args.port)
    try:
        m.call('new_level', {'discard_changes': True})
        total = 0
        for name, items in layers:
            placed = 0
            for k in range(0, len(items), 500):
                r = m.call('place_many', {'items': items[k:k + 500]})
                placed += r['placed']
                for n in r['nodes']:
                    if 'error' in n:
                        print(f'  {name}: item {k + n["index"]}: {n["error"]}')
            total += placed
            print(f'{name:22s} {placed:5d} elements')
        print(f'{total} elements in {time.time() - t0:.0f} s')
        if args.views:
            os.makedirs(args.views, exist_ok=True)
            for view, pos, look in [('district-aerial', [-300, 190, 260], [0, 20, -10]),
                                    ('district-yard', [118, 13, -12], [40, 22, -90])]:
                m.call('set_camera', {'position': pos, 'look_at': look})
                m.call('capture_view', {'width': 1280, 'height': 720}, image=os.path.join(args.views, view + '.png'))
        # the nav meshes of the ground connected to the streets, the yard and the catwalks
        points = [[x, 8.0, z] for x in (-120, 0, 120) for z in (-120, 0, 120)] + \
                 [[60, 8.0, -60], [0, 24.9, 60], [60, 24.9, 120]]
        r = m.call('generate_navmesh', {'region': {'min': [-230, 0, -230], 'max': [230, 30, 230]}, 'from': points})
        area = sum(n.get('area_m2', 0) for n in r['meshes'])
        print(f'nav meshes: {len(r["meshes"])}, {r["polygons"]} polygons, {area:.0f} m2, '
              f'{r["pieces_left_out"]} pieces of ground away from the streets left out')
        if args.nav:
            m.call('export_navmesh', {'path': os.path.abspath(args.nav)})
        os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
        m.call('save_project', {'path': os.path.abspath(args.out)})
        print('saved', args.out)
    finally:
        m.close()


if __name__ == '__main__':
    main()

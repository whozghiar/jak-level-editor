#!/usr/bin/env python3
"""Builds "indus-test", a small level in the style of Jak 3's industrial zone, to test a level of
the editor in the game: two streets crossing between four blocks of the zone's facades, closed at
their ends, three board vents of the zone (they throw Jak up when he rides over them on the
jetboard) at three heights, the nav meshes of the streets, and the collision the level builder
makes from the decor. It is built through the editor's MCP server, like
tools/build_industrial_district.py (whose modules of the zone it reuses), and exported for the
game.

Usage (from the repository root, after building the editor and extracting Jak 3):

  python tools/build_indus_test.py [--out build/demo/indus-test.glb] [--export <folder>]
                                   [--view <image.png>] [--port 47892]

With --export, the level is written to <folder>/indus-test/ (custom_assets/jak3/levels of a
jak-project checkout): see its INSTALL.md.
"""

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build_industrial_district as bid  # noqa: E402

NAME = 'indus-test'
CELLS = [-60, 60]  # the blocks, 120 m apart: the streets at x = 0 and z = 0, 40 m wide
EDGE = 104         # the level's edge: the outer faces of the blocks; the streets end against wall
                   # buildings there
VENT_AT = (994.156, 7.973, 1144.513)  # a board vent of ctyinda, its floor vent around it
# the board vents of the level: where, and how high they throw Jak (meters)
VENTS = [((0.0, 40.0), 12.0), ((40.0, 0.0), 20.0), ((0.0, -60.0), 28.0)]
START = [0.0, 9.5, -70.0]


class IndusTest(bid.District):
    def ground(self):
        # 4 x 4 slabs of the zone's ground, 52 m each: the ground ends at the edge (nothing to fall
        # from: the blocks and the walls close it)
        items = []
        size = 2 * EDGE / 4
        for ix in range(4):
            for iz in range(4):
                yaw = 90 if (ix * 7 + iz * 3) % 2 else 0
                x, z = -EDGE + size * (ix + 0.5), -EDGE + size * (iz + 0.5)
                items.append(self.part(bid.GROUND, x, bid.G, z, yaw, size / 64 * 1.001))
        return items

    def street_ends(self):
        """Two wall buildings back to back where each street meets the edge."""
        items = []
        n = 0
        for side in 'SNWE':
            inward = {'S': -90, 'N': 90, 'W': 0, 'E': 180}[side]
            for which in (0, 1):
                c = self.column(*bid.WALLS[(2 * n + which) % len(bid.WALLS)])
                lo, hi = c.zone.protos[c.bot]
                d = hi[0] if which == 0 else hi[0] - 2 * lo[0]
                o = {'S': (0, -EDGE + d), 'N': (0, EDGE - d), 'W': (-EDGE + d, 0), 'E': (EDGE - d, 0)}[side]
                yaw = inward + 180 if which == 0 else inward
                items += c.items(bid.mmul(bid.mmul(bid.tr(o[0], 8.01, o[1]), bid.ry(yaw)), bid.tr(0, 0, c.shift)))
            n += 1
        return items

    def vents(self):
        """The floor vent of a board vent of ctyinda (its grate and bars), where each vent goes."""
        x0, y0, z0 = VENT_AT
        grate = bid.Assembly(self.a, None, None, 0, where=lambda n, m: 'bigvent' in n and
                             abs(m[0][3] - x0) < 3 and abs(m[2][3] - z0) < 3)
        items = []
        for (x, z), _ in VENTS:
            items += grate.items(bid.mmul(bid.tr(x, bid.G, z), bid.tr(-x0, -y0, -z0)))
        return items

    def layers(self):
        out = [('ground', self.ground())]
        seed = 31
        for i, cx in enumerate(CELLS):
            for j, cz in enumerate(CELLS):
                kind = 'AB'[(i + j) % 2]
                seed += 1
                out.append((f'block {i},{j} ({kind})', self.block(cx, cz, kind, seed) + self.roof(cx, cz, kind, seed)))
        out += [('ends of the streets', self.street_ends()), ('floor vents', self.vents())]
        return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--out', default=os.path.join(bid.REPO, 'build', 'demo', NAME + '.glb'))
    ap.add_argument('--export', help='writes the level for the game to <folder>/indus-test/')
    ap.add_argument('--view', help='a picture of the level (capture_view)')
    ap.add_argument('--library', default=os.path.join(bid.REPO, 'library', 'jak3'))
    ap.add_argument('--port', type=int, default=47892)
    args = ap.parse_args()

    t0 = time.time()
    level = IndusTest(args.library)
    layers = level.layers()
    m = bid.Mcp(args.port)
    try:
        m.call('new_level', {'discard_changes': True})
        total = 0
        for name, items in layers:
            r = m.call('place_many', {'items': items})
            for n in r['nodes']:
                if 'error' in n:
                    print(f'  {name}: item {n["index"]}: {n["error"]}')
            total += r['placed']
            print(f'{name:22s} {r["placed"]:5d} elements')
        # the board vents: the zone's own actor on each floor vent, at its height
        for (x, z), height in VENTS:
            r = m.call('place', {'kind': 'object', 'name': 'industrial-board-vent', 'position': [x, bid.G, z]})
            vent = r['nodes'][0]['id']
            m.call('set_actor', {'id': vent, 'params': {'spring-height': height * 4096}})
            total += 1
        print(f'{total} elements in {time.time() - t0:.0f} s')
        # the nav meshes of the streets
        points = [[0, 8.0, z] for z in (-100, -50, 0, 50, 100)] + [[x, 8.0, 0] for x in (-100, -50, 50, 100)]
        r = m.call('generate_navmesh', {'region': {'min': [-EDGE, 0, -EDGE], 'max': [EDGE, 20, EDGE]}, 'from': points})
        print(f'nav meshes: {len(r["meshes"])}, {r["polygons"]} polygons')
        if args.view:
            m.call('set_camera', {'position': [-150, 120, -170], 'look_at': [0, 8, 0]})
            m.call('capture_view', {'width': 1280, 'height': 720}, image=args.view)
        os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
        m.call('save_project', {'path': os.path.abspath(args.out)})
        print('saved', args.out)
        if args.export:
            r = m.call('export_level', {'name': NAME, 'nickname': 'ixt', 'iso_name': 'INDUSTST',
                                        'folder': os.path.abspath(args.export), 'collision': 'decor',
                                        'start': START})
            print('exported', r['folder'], f'{r["meshes"]} meshes, {r["actors"]} actors, {r["navmeshes"]} nav meshes')
            for n in r['notes']:
                print('  ' + n['text'])
    finally:
        m.close()


if __name__ == '__main__':
    main()

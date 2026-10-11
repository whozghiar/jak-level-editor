#!/usr/bin/env python3
"""Builds the replacement demo of the tutorial (docs/captures/README.md): ctyindbis, the industrial
zone's ctyindb with one of its factory blocks taken down and tanks of the port and a floor vent put
in its place, made to replace ctyindb in the game, with the original shown in blue to compare. It drives
the editor through its remote control, as build_demo_level.py does.

It needs Jak 3 extracted with the editor, and holds no data of the game: only names and positions.

Usage (from the repository root, after building the editor):

  python tools/build_replace_demo.py [--out build/demo/ctyindbis.glb] [--port 47891]
"""

import argparse
import os
import sys

from build_demo_level import EDITOR, Editor

BLOCK = [1042, 12, 610]   # the middle of the factory block taken down, meters
HALF = [56, 21]           # its half length (x) and half width (z)


def build(e):
    e.call('open_level', name='ctyindb', discard_changes=True)
    # the block's decor (the ground stays: it is locked)
    found = e.call('find_nodes', kind='render', near=BLOCK, radius=62, limit=1000)['result']['nodes']
    inside = [n['id'] for n in found
              if abs(n['position'][0] - BLOCK[0]) < HALF[0] and abs(n['position'][2] - BLOCK[2]) < HALF[1]]
    e.call('delete_nodes', ids=inside)
    # tanks of the port and a floor vent of the industrial zone in its place, from the game's prefabs,
    # dropped on the ground
    e.call('list_prefabs')
    for name, x, z, yaw in [('Port big tank', -32, 0, 0), ('Port small tank', -8, 8, 0), ('Port small tank', -8, -9, 40),
                            ('Industrial floor vent', 24, 0, 0)]:
        e.call('place', kind='prefab', name=name, position=[BLOCK[0] + x, BLOCK[1], BLOCK[2] + z], on_ground=True,
               yaw_deg=yaw)
    # it replaces ctyindb: the original shown in blue, to compare
    e.call('replace_level', level='ctyindb', show_original=True)
    e.call('select', ids=[])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', default=os.path.join('build', 'demo', 'ctyindbis.glb'))
    ap.add_argument('--port', type=int, default=47891)
    a = ap.parse_args()
    if not os.path.isfile(EDITOR):
        raise SystemExit(f'build the editor first ({EDITOR} is missing)')
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    e = Editor(a.port)
    try:
        build(e)
        e.call('save_project', path=os.path.abspath(a.out))
    finally:
        e.close()
    print('saved', a.out)


if __name__ == '__main__':
    sys.exit(main())

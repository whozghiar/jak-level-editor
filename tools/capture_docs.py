#!/usr/bin/env python3
"""Makes the pictures of the documentation (README.md, docs/mcp-agent-guide.md) in
docs/captures/<feature>/: the editor takes them itself, in hidden windows. The pictures of a panel
or a window use the --screenshot setups (see `open-goal-level-editor --help`); those of a tool used
with the mouse drive a hidden editor through its remote control, as a person would: mouse events one
frame after the other (the tool ui_input), then a picture of the whole window. The interface is in
English.

Needs Jak 3 (and Jak II for its pictures) extracted with the editor, and the editor built. From the
repository root:

  python tools/capture_docs.py [--only getting-started,selecting,...] [--disc <your Jak 3 disc>]

The pictures show levels of Jak 3 extracted from a copy of the game. Jak 3 belongs to Sony
Interactive Entertainment and was developed by Naughty Dog.
"""

import argparse
import base64
import json
import os
import socket
import subprocess
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EDITOR = os.path.join(REPO, 'build', 'bin', 'open-goal-level-editor' + ('.exe' if os.name == 'nt' else ''))
OUT = os.path.join(REPO, 'docs', 'captures')


class Editor:
    """A hidden editor driven through its remote control: one JSON line per request and answer."""

    def __init__(self, port):
        self.proc = subprocess.Popen([EDITOR, '--hidden', '--lang', 'en', '--remote-port', str(port)],
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

    def call(self, tool, args=None, image=None, check=True):
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
        if image and a.get('image'):
            os.makedirs(os.path.dirname(image), exist_ok=True)
            with open(image, 'wb') as out:
                out.write(base64.b64decode(a['image']))
        return a.get('result')

    def ui(self, events, shot=None):
        if shot:
            events = events + [{'wait': 4}, {'shot': True}]
        return self.call('ui_input', {'events': events}, image=shot)

    def loaded(self):
        for _ in range(900):
            if not self.call('get_status')['document']['loading']:
                return
            time.sleep(0.2)

    def close(self):
        self.sock.close()
        self.proc.terminate()
        self.proc.wait(timeout=30)


def wait(n):
    return {'wait': n}


def click(x, y, frames=5):
    return [{'move': [x, y]}, wait(2), {'click': 'left'}, wait(frames)]


def out(feature, name):
    os.makedirs(os.path.join(OUT, feature), exist_ok=True)
    return os.path.join(OUT, feature, name)


def shot(level, feature, name, setup, *options):
    """A picture by a --screenshot setup of the editor."""
    path = out(feature, name)
    cmd = [EDITOR, '--screenshot', level, path, setup, '--lang', 'en'] + list(options)
    print(' '.join(cmd[1:]))
    # the temporary folders the pictures show (an export, an extraction): a neutral one
    tmp = os.path.join(REPO, 'build', 'tmp')
    os.makedirs(tmp, exist_ok=True)
    env = dict(os.environ, TEMP=tmp, TMP=tmp, TMPDIR=tmp)
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1800, env=env)


# where the interface is, in the 1600 x 900 window
TAB_LEVELS, TAB_BUILD, TAB_CATALOG, TAB_PREFABS, TAB_GROUPS = (38, 100), (86, 100), (143, 100), (207, 100), (265, 100)
MENU_EDIT, MENU_VIEW, MENU_LEVEL = (58, 14), (99, 14), (188, 14)
AWAY = {'move': [800, 880]}  # the cursor out of the view (the story bar)


def getting_started(args):
    shot('ctyport', 'getting-started', 'window.png', 'levels', '--game', '12', '--camera', '-150,140,1800,-40,-18')
    shot('ctyport', 'getting-started', 'guide.png', 'guide')
    if args.disc:
        shot(args.disc, 'getting-started', 'extraction.png', 'extract')


def levels(args):
    shot('ctysluma', 'levels', 'levels-panel.png', 'levels')
    shot('ctywide', 'levels', 'story.png', 'levels', '--camera', '250,420,800,36.25,-9.16', '--story',
         'tower-destroy-resolution')
    shot('ctyinda', 'levels', 'several-levels.png', 'levels', '--add', 'ctyindb,ctywide', '--camera',
         '1330,330,330,140.77,-26.11')
    shot('ctyinda', 'levels', 'level-selected.png', 'select', '--add', 'ctyindb', '--select', 'ctyindb')
    shot('ctyport', 'levels', 'water.png', 'levels', '--game', '12', '--camera', '60,30,1560,-52.4,-10.4')


roofs = []  # the highest element of each prefab placed last


def place_prefabs(e, names, at, step=34.0):
    """Places prefabs in a row on the ground from `at`; returns their groups (ids, the largest first)."""
    groups = []
    roofs.clear()
    for i, name in enumerate(names):
        p = e.call('place', {'kind': 'prefab', 'name': name, 'position': [at[0] + i * step, at[1], at[2]],
                             'on_ground': True})
        volume = lambda n: n.get('size', [0, 0, 0])[0] * n.get('size', [0, 0, 0])[1] * n.get('size', [0, 0, 0])[2]
        groups.append([n['id'] for n in sorted(p['nodes'], key=volume, reverse=True)])
        roofs.append(max(p['nodes'], key=lambda n: n.get('box', [[0, 0, 0], [0, 0, 0]])[1][1])['id'])
    return groups


def new_street(e):
    """A new level: a paved ground in the style of the slums, game rendering at 10 o'clock."""
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('place', {'kind': 'shape', 'name': 'ground', 'style': 'ctysluma', 'position': [0, 0, 0],
                     'size': [160, 0.5, 120], 'bumps': 0})
    e.call('set_view', {'game_view': True, 'hour': 10, 'weather': 'few_clouds'})


def selecting(args, e):
    if not args.quick:
        shot('wascitya', 'selecting', 'select.png', 'select')
        shot('ctyindb', 'selecting', 'scale.png', 'scale')
    new_street(e)
    groups = place_prefabs(e, ['main-town-building-a', 'main-town-building-b', 'haven-tree'], [-40, 0.5, 0])
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [-6, 70, 150], 'look_at': [-6, 10, 0]})
    # the cursor over an element of a prefab placed: its name, its prefab, how to select it all
    # (the editor waits a moment, in seconds, before it says: so does this)
    e.ui([{'move_to_node': groups[1][0]}, wait(5)])
    time.sleep(0.8)
    e.ui([wait(5)], shot=out('selecting', 'hover.png'))
    # a box being drawn: what it would select outlined, counted
    e.ui([{'move': [420, 300]}, wait(3), {'down': 'left'}, wait(3), {'move': [600, 420]}, wait(3), {'move': [760, 560]},
          wait(10)], shot=out('selecting', 'box.png'))
    e.ui([{'up': 'left'}, wait(5)])
    # the button held still on an element: a ring fills up, then its whole prefab is selected
    e.call('select', {'ids': []})
    e.ui([{'move_to_node': groups[0][0]}, wait(5), {'down': 'left'}, wait(2)])
    time.sleep(0.4)
    e.ui([wait(2)], shot=out('selecting', 'hold.png'))
    time.sleep(0.6)
    e.ui([wait(3), {'up': 'left'}, wait(5)])
    # the prefab selected whole, dragged to the side and up with the button held (from its roof: the
    # gizmo is in its middle)
    x, y = e.ui([{'move_to_node': roofs[0]}, wait(5)])['mouse']
    e.ui([{'down': 'left'}, wait(2), {'move': [x + 20, y - 4]}, wait(2), {'move': [x + 70, y - 30]}, wait(2),
          {'move': [x + 110, y - 50]}, wait(8)], shot=out('selecting', 'moving.png'))
    e.ui([{'up': 'left'}, wait(5)])



def groups_(args, e):
    new_street(e)
    place_prefabs(e, ['main-town-building-c', 'main-town-building-d'], [-40, 0.5, -10], step=40)
    square = place_prefabs(e, ['small-fountain', 'haven-tree', 'haven-tree'], [-20, 0.5, 35], step=14)
    ids = [i for g in square for i in g]
    g = e.call('group', {'ids': ids, 'name': 'Little square'})
    e.call('select', {'group': g['group']})
    e.call('set_camera', {'position': [10, 55, 120], 'look_at': [-10, 5, 10]})
    e.ui(click(*TAB_GROUPS) + [AWAY, wait(10)], shot=out('groups', 'panel.png'))
    # a group of the selection named: Ctrl + Shift + G
    e.call('select', {'ids': square[1] + square[2]})
    e.ui([{'move': [800, 450]}, wait(2), {'key': 'G', 'mods': ['ctrl', 'shift']}, wait(20), {'key': 'A', 'mods': ['ctrl']},
          wait(10), {'key': 'Backspace'}, wait(10), {'text': 'Two trees'}, wait(20)], shot=out('groups', 'new-group.png'))
    e.ui([{'key': 'Escape'}, wait(4)])


def catalog(args, e):
    if not args.quick:
        shot('ctyinda', 'catalog', 'objects.png', 'catalog')
        shot('wascitya', 'catalog', 'models.png', 'models')
        shot('ctyindb', 'catalog', 'decor-search.png', 'decor-all')
        shot('ctyindb', 'catalog', 'materials.png', 'materials')
    e.call('open_level', {'name': 'ctyinda', 'actors': False, 'discard_changes': True})
    e.loaded()
    e.call('list_catalog', {'limit': 1})  # the levels read for the catalog
    # the interactive objects
    e.ui(click(*TAB_CATALOG) + click(104, 172, 10) + [AWAY, wait(120)], shot=out('catalog', 'interactive.png'))
    # the visual effects, their kinds
    e.ui(click(202, 172, 10) + [AWAY, wait(120)] + click(168, 312, 10), shot=out('catalog', 'effects.png'))
    e.ui([{'key': 'Escape'}, wait(3)])


def prefabs(args, e):
    e.call('open_level', {'name': 'ctyinda', 'actors': False, 'discard_changes': True})
    e.loaded()
    e.ui(click(*TAB_PREFABS) + [AWAY, wait(200)], shot=out('prefabs', 'panel.png'))
    e.ui(click(160, 265) + [{'text': 'gate'}, wait(150)], shot=out('prefabs', 'search.png'))
    # a house of Spargus selected whole (its assembly), made a prefab: the small pieces left out
    e.call('open_level', {'name': 'wascitya', 'actors': False, 'discard_changes': True})
    e.loaded()
    houses = e.call('list_assemblies', {'min_elements': 40, 'limit': 60})['assemblies']
    house = min(houses, key=lambda a: abs(max(a['box'][1][k] - a['box'][0][k] for k in (0, 2)) - 35))
    e.call('select', {'assembly': house['ids'][0]})
    e.call('set_camera', {'frame': house['ids'], 'yaw_deg': 30, 'pitch_deg': -32})
    e.ui(click(*TAB_PREFABS) + [AWAY, wait(30)], shot=out('prefabs', 'assembly.png'))
    e.ui(click(827, 53, 10), shot=out('prefabs', 'create.png'))
    e.ui([{'key': 'Escape'}, wait(4)])
    # a prefab of the user's being placed: its picture follows the cursor
    new_street(e)
    e.call('set_camera', {'position': [0, 60, 110], 'look_at': [0, 0, 0]})
    e.ui(click(*TAB_PREFABS) + [wait(60)])
    e.ui([{'move': [700, 520]}, wait(10)])


def building(args, e):
    if not args.quick:
        shot('ctyindb', 'building', 'shapes.png', 'build')
    # a fence of the factory broken in the preview, mid-flight
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    r = e.call('place', {'kind': 'object', 'name': 'fac-break-fence-a', 'position': [0, 0, 0]})
    e.call('select', {'ids': [r['nodes'][0]['id']]})
    e.call('set_camera', {'position': [20, 13, 33], 'look_at': [0, 5, 0]})
    e.ui(click(1318, 254, 22), shot=out('building', 'preview.png'))
    # a floor's faces pulled out with the topology tool
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    r = e.call('place', {'kind': 'shape', 'name': 'floor', 'position': [0, 0, 0], 'size': [8, 0.5, 8]})
    e.call('select', {'ids': [r['nodes'][0]['id']]})
    e.call('set_camera', {'position': [0, 12, 12], 'look_at': [0, 0, 0]})
    e.ui([{'move': [800, 450]}, wait(5), {'key': 'T'}, wait(10)] + click(1385, 202) + click(700, 430, 10) +
         click(1325, 324, 10) + click(1320, 291, 10) + [{'move': [800, 760]}, wait(10)], shot=out('building', 'topology.png'))
    e.ui([{'key': 'T'}, wait(5)])
    # a yard of the port: a floor and walls in the style of ctyport, one of them bent
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('place', {'kind': 'shape', 'name': 'ground', 'style': 'ctyport', 'position': [0, 0, 0], 'size': [24, 0.5, 24]})
    e.call('place', {'kind': 'shape', 'name': 'wall', 'style': 'ctyport', 'position': [0, 0, -12], 'size': [24, 6, 0.5]})
    e.call('place', {'kind': 'shape', 'name': 'wall', 'style': 'ctyport', 'position': [-12, 0, 0], 'size': [24, 6, 0.5],
                     'yaw_deg': 90, 'bend': -60})
    e.call('place', {'kind': 'shape', 'name': 'pillar', 'style': 'ctyport', 'position': [-4, 0, 6], 'size': [2, 5, 2],
                     'round': 1})
    e.call('place', {'kind': 'shape', 'name': 'stairs', 'style': 'ctyport', 'position': [4, 0.5, 2], 'size': [3, 4, 9],
                     'steps': 12, 'bend': 120, 'yaw_deg': 180})
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [20, 18, 26], 'look_at': [0, 1, -2]})
    e.ui(click(*TAB_BUILD) + click(168, 532) + [{'text': 'ctyport'}, wait(10)] + click(100, 584, 300) +
         [AWAY, wait(5)], shot=out('building', 'style.png'))
    # natural ground drawn with the terrain brush in the style of the desert
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    t = e.call('terrain', {'mode': 'generate', 'position': [0, 0, 0], 'area': [96, 96], 'hills': 14,
                           'roughness': 0.55, 'style': 'desertb', 'seed': 4})['id']
    e.call('terrain', {'id': t, 'mode': 'dig', 'position': [-40, 0, -10], 'to': [40, 0, 15], 'size': 10, 'amount': 3})
    e.call('terrain', {'id': t, 'mode': 'flatten', 'position': [20, 6, -25], 'size': 22, 'amount': 1, 'height': 6})
    for a, b in (([10, 6, -35], [30, 6, -35]), ([30, 6, -35], [30, 6, -15]), ([30, 6, -15], [10, 6, -15]),
                 ([10, 6, -15], [10, 6, -30])):
        e.call('terrain', {'id': t, 'mode': 'wall', 'position': a, 'to': b, 'size': 1.5, 'amount': 3})
    e.call('terrain', {'id': t, 'mode': 'raise', 'position': [-25, 0, 25], 'size': 20, 'amount': 10})
    e.call('select', {'ids': [t]})
    e.call('set_camera', {'position': [62, 40, 62], 'look_at': [0, 0, 0]})
    e.ui(click(*TAB_BUILD) + [{'move': [800, 450]}, wait(3), {'key': 'B'}, wait(5)] + [{'key': ']'}, wait(2)] * 6 +
         [{'move': [700, 520]}, wait(20)], shot=out('building', 'terrain.png'))
    e.ui([{'key': 'B'}, wait(3)])
    # the atoll, a level of Jak II, opened in the game rendering
    if 'error' not in e.call('list_levels', {'game': 'jak2'}, check=False):
        e.call('new_level', {'discard_changes': True})
        e.ui(click(*TAB_LEVELS) + click(121, 133) + click(170, 166) + [{'text': 'atoll'}, wait(10)] +
             [{'move': [60, 225]}, wait(2), {'click': 'left'}, {'click': 'left'}, wait(30)])
        e.loaded()
        e.call('set_view', {'game_view': True, 'hour': 11, 'weather': 'few_clouds'}, check=False)
        e.ui([AWAY, {'key': 'Home'}, wait(60)], shot=out('building', 'jak2-level.png'))
        # back to Jak 3's levels, the filter emptied
        e.ui(click(*TAB_LEVELS) + click(66, 133) + click(168, 166) + [{'key': 'A', 'mods': ['ctrl']}, wait(3),
             {'key': 'Backspace'}, wait(3), {'key': 'Escape'}, wait(3)])


FACADE = 'ind-facade-60'


def closing(args, e):
    new_street(e)
    g = place_prefabs(e, [FACADE], [0, 0.5, 0])[0]
    box = e.call('list_groups')['groups'][-1]['box']
    c = [(box[0][k] + box[1][k]) / 2 for k in range(3)]
    d = max(box[1][k] - box[0][k] for k in range(3)) * 1.1
    e.call('select', {'ids': []})
    # its front is the side of its alcove (+x): the street; from behind and from the side, what the
    # player should never see
    views = {'front': [c[0] + d, c[1] + d * 0.2, c[2] + d * 0.15], 'back': [c[0] - d, c[1] + d * 0.35, c[2] + d * 0.5],
             'side': [c[0] + d * 0.2, c[1] + d * 0.3, c[2] - d]}
    for name, at in views.items():
        e.call('set_camera', {'position': at, 'look_at': c})
        e.call('capture_view', {'width': 960, 'height': 560}, image=out('closing-buildings', 'before-%s.png' % name))
    # closed as seen from the street in front of it
    r = e.call('close_building', {'ids': g, 'seen_from': views['front']})
    print('closure:', {k: r[k] for k in ('facade', 'cells', 'cells_added', 'cell', 'yaw_deg', 'materials')})
    for name, at in views.items():
        e.call('set_camera', {'position': at, 'look_at': c})
        e.call('capture_view', {'width': 960, 'height': 560}, image=out('closing-buildings', 'after-%s.png' % name))
    # the menu
    e.call('undo')
    e.call('select', {'ids': g})
    e.call('set_camera', {'position': views['front'], 'look_at': c})
    e.ui(click(*MENU_EDIT, 6) + [{'move': [160, 330]}, wait(10)], shot=out('closing-buildings', 'menu.png'))
    e.ui([{'key': 'Escape'}, wait(3)])


def holes(args, e):
    # a yard whose floor has a slit and whose wall stops short of the corner
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    style = {'kind': 'shape', 'style': 'ctyport'}
    e.call('place', dict(style, name='floor', position=[-10.6, 0, 0], size=[20, 0.5, 30]))
    e.call('place', dict(style, name='floor', position=[10.6, 0, 0], size=[20, 0.5, 30]))
    e.call('place', dict(style, name='wall', position=[0, 0.5, -15], size=[42, 6, 0.5]))
    e.call('place', dict(style, name='wall', position=[-20.8, 0.5, 0], size=[30, 6, 0.5], yaw_deg=90))
    e.call('place', dict(style, name='wall', position=[20.8, 0.5, 3], size=[24, 6, 0.5], yaw_deg=90))
    e.call('set_view', {'game_view': True, 'hour': 10, 'weather': 'clear'})
    r = e.call('find_gaps', {'box': [[-20, -1, -14], [20, 6, 14]], 'spacing': 3})
    print('gaps:', {k: r[k] for k in ('gaps', 'edges', 'viewpoints')})
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [30, 26, 40], 'look_at': [0, 0, 0]})
    e.ui([AWAY, wait(20)], shot=out('holes', 'window.png'))
    e.call('capture_view', {'width': 960, 'height': 560}, image=out('holes', 'assistant-view.png'))
    e.ui(click(442, 73, 5))  # the window closed (its cross)


GAME_WINDOW = [click(*MENU_VIEW, 6), click(150, 119, 10)]


def game_rendering(args, e):
    if not args.quick:
        shot('ctyindb', 'game-rendering', 'effects.png', 'actors', '--game', '12', '--camera',
             '1131.75,37.85,589.0,225,-14')
    e.call('open_level', {'name': 'ctysluma', 'actors': False, 'discard_changes': True})
    e.loaded()
    lo, hi = e.call('get_status')['document']['bounds']['min'], e.call('get_status')['document']['bounds']['max']
    mid = [(lo[k] + hi[k]) / 2 for k in range(3)]
    e.call('set_camera', {'position': [mid[0], hi[1] + 25, hi[2] + 40], 'look_at': [mid[0], hi[1] - 10, mid[2]]})
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'clear'})
    e.ui(GAME_WINDOW[0] + GAME_WINDOW[1] + [AWAY, wait(20)], shot=out('game-rendering', 'noon-clear.png'))
    for name, hour, weather in (('dusk', 18.3, 'few_clouds'), ('storm', 15, 'storm_rain'), ('night', 22.5, 'clear'),
                                ('haze', 7.5, 'haze')):
        e.call('set_view', {'hour': hour, 'weather': weather})
        e.ui([AWAY, wait(20)], shot=out('game-rendering', '%s.png' % name))
    e.call('set_view', {'hour': 12, 'weather': 'few_clouds'})
    e.ui([{'key': 'Escape'}, wait(3)])


def behaviors(args, e):
    e.call('open_level', {'name': 'ctyinda', 'actors': True, 'discard_changes': True})
    e.loaded()
    found = e.call('find_nodes', {'etype': 'com-airlock-outer', 'limit': 5})['nodes']
    crates = e.call('find_nodes', {'etype': 'crate', 'limit': 5})['nodes']
    if found and crates:
        door = found[0]
        e.call('set_actor', {'id': door['id'], 'behavior': {'open': 'when', 'distance': 10,
                                                            'when': {'when': 'gone', 'arg': crates[0]['name']}}})
        e.call('select', {'ids': [door['id']]})
        # in front of the door, outside its walls (framing it puts the camera inside them)
        c = [door['position'][0], door['position'][1] + 5, door['position'][2]]
        e.call('set_camera', {'position': [c[0] + 16, c[1] + 4, c[2]], 'look_at': c})
        e.ui([AWAY, wait(30)], shot=out('behaviors', 'airlock.png'))


def exporting(args, e):
    if not args.quick:
        shot('ctysluma', 'export', 'export-window.png', 'export')
        shot('ctysluma', 'nav-meshes', 'navmesh.png', 'navmesh')
    e.call('open_level', {'name': 'ctyindb', 'actors': False, 'discard_changes': True})
    e.loaded()
    # made a replacement of ctyindb, the original shown in blue to compare
    e.call('replace_level', {'level': 'ctyindb', 'show_original': True})
    e.loaded()
    e.call('select', {'ids': []})
    e.ui([AWAY, wait(30)], shot=out('export', 'replace.png'))


def assistant(args, e):
    # what an assistant builds and sees: a street of Jak II's Main Town on paving of the slums, the
    # buildings closed at the back, looked at from the player's height
    new_street(e)
    names = ['main-town-building-a', 'main-town-building-c', 'main-town-building-d']
    groups = place_prefabs(e, names, [-45, 0.5, -20], step=42)
    for g in groups:
        e.call('close_building', {'ids': g, 'seen_from': [0, 1.6, 40]})
    place_prefabs(e, ['small-fountain'], [0, 0.5, 18])
    place_prefabs(e, ['haven-tree', 'haven-tree'], [-25, 0.5, 30], step=50)
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [8, 1.6 + 0.5, 48], 'look_at': [-4, 10, -10]})
    e.call('capture_view', {'width': 960, 'height': 560, 'game_hour': 16}, image=out('mcp', 'assistant-view.png'))
    e.call('set_camera', {'position': [60, 70, 110], 'look_at': [0, 5, -10]})
    e.call('capture_view', {'width': 960, 'height': 560, 'game_hour': 16}, image=out('mcp', 'street-above.png'))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--port', type=int, default=47896)
    ap.add_argument('--only', default='')
    ap.add_argument('--quick', action='store_true', help='only the pictures made through the remote control')
    ap.add_argument('--disc', default='', help='your Jak 3 disc (.iso or folder) for the extraction window')
    args = ap.parse_args()
    only = set(filter(None, args.only.split(',')))
    want = lambda f: not only or f in only
    if want('getting-started'):
        getting_started(args)
    if want('levels'):
        levels(args)
    e = None
    try:
        for name, f in (('selecting', selecting), ('groups', groups_), ('catalog', catalog), ('prefabs', prefabs),
                        ('building', building), ('closing-buildings', closing), ('holes', holes),
                        ('game-rendering', game_rendering), ('behaviors', behaviors), ('export', exporting),
                        ('mcp', assistant)):
            if want(name):
                e = e or Editor(args.port)
                f(args, e)
    finally:
        if e:
            e.close()


if __name__ == '__main__':
    main()

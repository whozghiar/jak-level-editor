#!/usr/bin/env python3
"""Makes the pictures of the documentation (README.md, docs/mcp-agent-guide.md) in
docs/captures/<feature>/. It starts the editor (target/release or target/debug jak-level-editor,
built before) and drives it two ways:

- its remote control (the MCP server's tools: open a level, place, set the camera and the view...)
  sets up each scene, and `capture_view` gives a picture of the 3D view alone;
- the DevTools protocol of its web view (WebView2, Windows) gives a picture of the whole window
  (Page.captureScreenshot), clicks a tab or a menu by its English label, presses keys, and makes any
  call of the engine as the interface does.

What the C++ editor's ui_input did with the mouse over the 3D view (the cursor's label over an
element, a box being drawn, the button held, a drag, the terrain brush, the topology tool) is not
done here: those pictures (MANUAL below) are taken by hand. Needs Jak 3 (and Jak II for its
pictures) extracted with the editor, and the prefabs of yours the pictures use
(main-town-building-a to -d, small-fountain, haven-tree, ind-facade-60). From the repository root:

  python tools/capture_docs.py [--only getting-started,levels,...] [--out <folder>] [--editor <program>]

The pictures show levels of Jak 3 extracted from a copy of the game. Jak 3 belongs to Sony
Interactive Entertainment and was developed by Naughty Dog.
"""

import argparse
import base64
import json
import os
import socket
import struct
import subprocess
import tempfile
import time
import urllib.parse
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = 'jak-level-editor' + ('.exe' if os.name == 'nt' else '')
OUT = os.path.join(REPO, 'docs', 'captures')
SIZE = (1600, 900)

# the pictures of the README this script does not take (the mouse over the 3D view, a disc)
MANUAL = [
    'getting-started/extraction.png (the extraction window after a disc was extracted)',
    'selecting/hover.png, box.png, hold.png, moving.png (the cursor over a prefab, a box being drawn,'
    ' the button held on a prefab, a prefab dragged)',
    'groups/new-group.png (Ctrl+Shift+G, a group being named)',
    'prefabs/create.png (the window of a prefab being made)',
    'building/preview.png, topology.png, terrain.png, terrain-raise.png, terrain-magnet.png,'
    ' terrain-magnet-held.png (the tools used with the mouse)',
]


def find_editor():
    for profile in ('release', 'debug'):
        p = os.path.join(REPO, 'target', profile, EXE)
        if os.path.isfile(p):
            return p
    raise SystemExit('build the editor first: npx vite build, then cargo build --release -p jak-level-editor')


class Editor:
    """The editor started with its remote control on `port` and its web view's DevTools on
    `devtools`, a WebView2 data folder of its own (one shared with another running editor would keep
    its window from appearing)."""

    def __init__(self, exe, port, devtools):
        env = dict(os.environ, WEBVIEW2_USER_DATA_FOLDER=os.path.join(tempfile.gettempdir(), 'jle-captures-wv2'),
                   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=%d' % devtools)
        self.proc = subprocess.Popen([exe, '--lang', 'en', '--remote-port', str(port)], env=env,
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
        self.web = DevTools(devtools)
        self.web.send('Emulation.setDeviceMetricsOverride', width=SIZE[0], height=SIZE[1], deviceScaleFactor=1,
                      mobile=False)
        # the interface answers capture_view once its view is up
        for _ in range(300):
            if self.web.evaluate('!!document.querySelector("canvas")'):
                break
            time.sleep(0.2)
        for _ in range(10):
            if 'error' not in self.call('capture_view', {'width': 64, 'height': 64}, check=False):
                break
            time.sleep(0.5)

    def call(self, tool, args=None, image=None, check=True):
        """A tool of the remote control (the MCP server's)."""
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

    def loaded(self):
        for _ in range(900):
            if not self.call('get_status')['document']['loading']:
                return
            time.sleep(0.2)

    def shot(self, path, settle=1.0):
        """A picture of the whole window, once the view had `settle` seconds to draw."""
        time.sleep(settle)
        self.web.shot(path)

    def close(self):
        self.sock.close()
        self.proc.terminate()
        self.proc.wait(timeout=30)


class DevTools:
    """The Chrome DevTools protocol of the web view, over the smallest WebSocket client that does."""

    def __init__(self, port):
        for _ in range(300):
            try:
                with urllib.request.urlopen('http://127.0.0.1:%d/json' % port, timeout=1) as r:
                    page = next(p for p in json.load(r) if p['type'] == 'page')
                break
            except (OSError, ValueError, StopIteration):
                time.sleep(0.2)
        else:
            raise SystemExit('no DevTools on port %d (WebView2 only)' % port)
        url = urllib.parse.urlparse(page['webSocketDebuggerUrl'])
        self.sock = socket.create_connection((url.hostname, url.port), timeout=300)
        self.sock.sendall(('GET %s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n'
                           'Sec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n'
                           % (url.path, url.netloc, base64.b64encode(os.urandom(16)).decode())).encode())
        self.f = self.sock.makefile('rb')
        while self.f.readline() not in (b'\r\n', b''):
            pass
        self.n = 0

    def _frame(self, data):
        n = len(data)
        head = bytes([0x81, 0x80 | n]) if n < 126 else (bytes([0x81, 0xFE]) + struct.pack('>H', n) if n < 65536
                                                         else bytes([0x81, 0xFF]) + struct.pack('>Q', n))
        mask = os.urandom(4)
        self.sock.sendall(head + mask + bytes(b ^ mask[i & 3] for i, b in enumerate(data)))

    def _message(self):
        data = b''
        while True:
            b0, b1 = self.f.read(2)
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack('>H', self.f.read(2))[0]
            elif n == 127:
                n = struct.unpack('>Q', self.f.read(8))[0]
            data += self.f.read(n)
            if b0 & 0x80:
                return data

    def send(self, method, **params):
        self.n += 1
        self._frame(json.dumps({'id': self.n, 'method': method, 'params': params}).encode())
        while True:
            m = json.loads(self._message())
            if m.get('id') == self.n:
                if 'error' in m:
                    raise SystemExit('%s: %s' % (method, m['error']))
                return m.get('result', {})

    def evaluate(self, expression):
        r = self.send('Runtime.evaluate', expression=expression, awaitPromise=True, returnByValue=True)
        if 'exceptionDetails' in r:
            raise SystemExit('%s: %s' % (expression[:80], r['exceptionDetails'].get('text')))
        return r.get('result', {}).get('value')

    def engine(self, name, args=None):
        """A call of the engine, as the interface makes it (crates/core/src/editor/api.rs)."""
        return self.evaluate('window.__TAURI_INTERNALS__.invoke("plugin:jak-level-editor|call", %s)'
                             % json.dumps({'name': name, 'args': args or {}}))

    def click(self, label):
        """The first shown button or entry whose text or aria-label is `label` (the interface in English)."""
        found = self.evaluate('''(() => {
          const e = [...document.querySelectorAll("button, [role=tab], [role=menuitem], a, label")]
            .find((e) => e.offsetParent && [e.getAttribute("aria-label"), e.textContent.trim()].includes(%s));
          if (e) e.click();
          return !!e;
        })()''' % json.dumps(label))
        if not found:
            raise SystemExit('nothing labelled %r to click' % label)
        time.sleep(0.3)

    def close_windows(self):
        """The editor's floating windows closed (their own Close, not the editor's)."""
        self.evaluate('document.querySelectorAll("[role=dialog] > header button").forEach((b) => b.click())')
        time.sleep(0.3)

    def key(self, key, ctrl=False, shift=False):
        """A key pressed, for the interface's shortcuts (ui/lib/shortcuts.ts)."""
        self.evaluate('window.dispatchEvent(new KeyboardEvent("keydown", %s))'
                      % json.dumps({'key': key, 'ctrlKey': ctrl, 'shiftKey': shift, 'bubbles': True}))
        time.sleep(0.3)

    def shot(self, path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'wb') as out:
            out.write(base64.b64decode(self.send('Page.captureScreenshot', format='png')['data']))


def out(feature, name):
    return os.path.join(OUT, feature, name)


def open_level(e, name, actors=False, add=()):
    e.call('open_level', {'name': name, 'actors': actors, 'discard_changes': True})
    e.loaded()
    for level in add:
        e.call('add_level', {'name': level})
        e.loaded()
    e.call('select', {'ids': []})


def camera(e, x, y, z, yaw, pitch):
    e.call('set_camera', {'position': [x, y, z], 'yaw_deg': yaw, 'pitch_deg': pitch})


def getting_started(args, e):
    open_level(e, 'ctyport')
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'few_clouds'})
    camera(e, -150, 140, 1800, -40, -18)
    e.web.click('Levels')
    e.shot(out('getting-started', 'window.png'), settle=3)
    e.web.key('F1')
    e.shot(out('getting-started', 'guide.png'))
    e.web.close_windows()


def levels(args, e):
    open_level(e, 'ctysluma')
    e.web.click('Levels')
    e.shot(out('levels', 'levels-panel.png'), settle=3)
    open_level(e, 'ctywide')
    states = e.call('get_status').get('story', {}).get('states', 0)
    if isinstance(states, list):
        states = len(states)
    if states:
        e.call('set_story', {'enabled': True, 'state': states})  # the last: after the tower's destruction
    camera(e, 250, 420, 800, 36.25, -9.16)
    e.shot(out('levels', 'story.png'), settle=3)
    open_level(e, 'ctyinda', add=['ctyindb', 'ctywide'])
    camera(e, 1330, 330, 330, 140.77, -26.11)
    e.shot(out('levels', 'several-levels.png'), settle=3)
    open_level(e, 'ctyinda', add=['ctyindb'])
    e.call('select', {'level': 'ctyindb'})
    e.call('set_camera', {'level': 'ctyindb'})
    e.shot(out('levels', 'level-selected.png'), settle=3)
    open_level(e, 'ctyport')
    e.call('set_view', {'game_view': True, 'hour': 12})
    camera(e, 60, 30, 1560, -52.4, -10.4)
    e.shot(out('levels', 'water.png'), settle=3)


def place_prefabs(e, names, at, step=34.0):
    """Places prefabs in a row on the ground from `at`; returns their groups (ids, the largest first)."""
    groups = []
    for i, name in enumerate(names):
        p = e.call('place', {'kind': 'prefab', 'name': name, 'position': [at[0] + i * step, at[1], at[2]],
                             'on_ground': True})
        volume = lambda n: n.get('size', [0, 0, 0])[0] * n.get('size', [0, 0, 0])[1] * n.get('size', [0, 0, 0])[2]
        groups.append([n['id'] for n in sorted(p['nodes'], key=volume, reverse=True)])
    return groups


def new_street(e):
    """A new level: a paved ground in the style of the slums, game rendering at 10 o'clock."""
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('place', {'kind': 'shape', 'name': 'ground', 'style': 'ctysluma', 'position': [0, 0, 0],
                     'size': [160, 0.5, 120], 'bumps': 0})
    e.call('set_view', {'game_view': True, 'hour': 10, 'weather': 'few_clouds'})


def selecting(args, e):
    # a building of Spargus selected, the move gizmo on it; a bridge of the industrial zone and the
    # scale gizmo
    open_level(e, 'wascitya')
    house = max(e.call('list_assemblies', {'min_elements': 40, 'limit': 20})['assemblies'], key=lambda a: len(a['ids']))
    e.call('select', {'ids': house['ids']})
    e.call('set_camera', {'frame': house['ids'], 'yaw_deg': 30, 'pitch_deg': -25})
    e.web.key('w')
    e.shot(out('selecting', 'select.png'), settle=3)
    open_level(e, 'ctyindb')
    bridge = e.call('find_nodes', {'name': 'bridge', 'limit': 1})['nodes']
    if bridge:
        e.call('select', {'ids': [bridge[0]['id']]})
        e.call('set_camera', {'frame': [bridge[0]['id']], 'yaw_deg': 35, 'pitch_deg': -25})
        e.web.key('r')
        e.shot(out('selecting', 'scale.png'), settle=3)
        e.web.key('w')


def groups_(args, e):
    new_street(e)
    place_prefabs(e, ['main-town-building-c', 'main-town-building-d'], [-40, 0.5, -10], step=40)
    square = place_prefabs(e, ['small-fountain', 'haven-tree', 'haven-tree'], [-20, 0.5, 35], step=14)
    g = e.call('group', {'ids': [i for g in square for i in g], 'name': 'Little square'})
    e.call('select', {'group': g['group']})
    e.call('set_camera', {'position': [10, 55, 120], 'look_at': [-10, 5, 10]})
    e.web.click('Groups')
    e.shot(out('groups', 'panel.png'), settle=3)


def catalog(args, e):
    open_level(e, 'ctyinda')
    e.call('list_catalog', {'limit': 1})  # the levels read for the catalog
    e.web.click('Catalog')
    for tab, name in (('Objects', 'objects.png'), ('Interactive', 'interactive.png'), ('Effects', 'effects.png'),
                      ('Models', 'models.png'), ('Decor', 'decor-search.png'), ('Materials', 'materials.png')):
        e.web.click(tab)
        e.shot(out('catalog', name), settle=4)


def prefabs(args, e):
    open_level(e, 'ctyinda')
    e.web.click('Prefabs')
    e.shot(out('prefabs', 'panel.png'), settle=5)
    # a house of Spargus selected whole (its assembly)
    open_level(e, 'wascitya')
    houses = e.call('list_assemblies', {'min_elements': 40, 'limit': 60})['assemblies']
    house = min(houses, key=lambda a: abs(max(a['box'][1][k] - a['box'][0][k] for k in (0, 2)) - 35))
    e.call('select', {'assembly': house['ids'][0]})
    e.call('set_camera', {'frame': house['ids'], 'yaw_deg': 30, 'pitch_deg': -32})
    e.shot(out('prefabs', 'assembly.png'), settle=3)


def building(args, e):
    # a yard of the port: a floor and walls in the style of ctyport, one of them bent
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    style = {'kind': 'shape', 'style': 'ctyport'}
    e.call('place', dict(style, name='ground', position=[0, 0, 0], size=[24, 0.5, 24]))
    e.call('place', dict(style, name='wall', position=[0, 0, -12], size=[24, 6, 0.5]))
    e.call('place', dict(style, name='wall', position=[-12, 0, 0], size=[24, 6, 0.5], yaw_deg=90, bend=-60))
    e.call('place', dict(style, name='pillar', position=[-4, 0, 6], size=[2, 5, 2], round=1))
    e.call('place', dict(style, name='stairs', position=[4, 0.5, 2], size=[3, 4, 9], steps=12, bend=120, yaw_deg=180))
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [20, 18, 26], 'look_at': [0, 1, -2]})
    e.web.click('Build')
    e.shot(out('building', 'shapes.png'), settle=3)
    e.shot(out('building', 'style.png'))
    # the atoll, a level of Jak II, opened in the game rendering
    if 'error' not in e.call('list_levels', {'game': 'jak2'}, check=False):
        e.call('open_level', {'name': 'atoll', 'game': 'jak2', 'actors': False, 'discard_changes': True})
        e.loaded()
        e.call('set_view', {'game_view': True, 'hour': 11, 'weather': 'few_clouds'}, check=False)
        e.call('set_camera', {'all': True})
        e.shot(out('building', 'jak2-level.png'), settle=4)


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
    print('gaps:', {k: r.get(k) for k in ('gaps', 'edges', 'viewpoints')})
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [30, 26, 40], 'look_at': [0, 0, 0]})
    e.shot(out('holes', 'window.png'), settle=3)
    e.call('capture_view', {'width': 960, 'height': 560}, image=out('holes', 'assistant-view.png'))


def game_rendering(args, e):
    open_level(e, 'ctysluma')
    status = e.call('get_status')['document']
    lo, hi = status['bounds']['min'], status['bounds']['max']
    mid = [(lo[k] + hi[k]) / 2 for k in range(3)]
    e.call('set_camera', {'position': [mid[0], hi[1] + 25, hi[2] + 40], 'look_at': [mid[0], hi[1] - 10, mid[2]]})
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'clear'})
    e.web.click('View')
    e.web.click('Sky, hour and weather...')
    e.shot(out('game-rendering', 'noon-clear.png'), settle=4)
    for name, hour, weather in (('dusk', 18.3, 'few_clouds'), ('storm', 15, 'storm_rain'), ('night', 22.5, 'clear'),
                                ('haze', 7.5, 'haze')):
        e.call('set_view', {'hour': hour, 'weather': weather})
        e.shot(out('game-rendering', '%s.png' % name), settle=3)
    e.call('set_view', {'hour': 12, 'weather': 'few_clouds'})
    e.web.close_windows()
    # the effects of the industrial zone running around the objects that make them
    open_level(e, 'ctyindb', actors=True)
    e.call('set_view', {'game_view': True, 'hour': 12, 'effects': True}, check=False)
    camera(e, 1131.75, 37.85, 589.0, 225, -14)
    e.shot(out('game-rendering', 'effects.png'), settle=4)


def behaviors(args, e):
    open_level(e, 'ctyinda', actors=True)
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
        e.shot(out('behaviors', 'airlock.png'), settle=3)


def exporting(args, e):
    open_level(e, 'ctysluma')
    e.call('generate_navmesh', {})
    e.loaded()
    e.shot(out('nav-meshes', 'navmesh.png'), settle=3)
    e.web.click('Level')
    e.web.click('Export for the game...')
    e.shot(out('export', 'export-window.png'))
    e.web.close_windows()
    # made a replacement of ctyindb, the original shown in blue to compare
    open_level(e, 'ctyindb')
    e.call('replace_level', {'level': 'ctyindb', 'show_original': True})
    e.loaded()
    e.call('select', {'ids': []})
    e.shot(out('export', 'replace.png'), settle=3)


def assistant(args, e):
    # what an assistant builds and sees: a street of Jak II's Main Town on paving of the slums,
    # looked at from the player's height
    new_street(e)
    place_prefabs(e, ['main-town-building-a', 'main-town-building-c', 'main-town-building-d'], [-45, 0.5, -20], step=42)
    place_prefabs(e, ['small-fountain'], [0, 0.5, 18])
    place_prefabs(e, ['haven-tree', 'haven-tree'], [-25, 0.5, 30], step=50)
    e.call('select', {'ids': []})
    e.call('set_camera', {'position': [8, 1.6 + 0.5, 48], 'look_at': [-4, 10, -10]})
    e.call('capture_view', {'width': 960, 'height': 560, 'game_hour': 16}, image=out('mcp', 'assistant-view.png'))
    e.call('set_camera', {'position': [60, 70, 110], 'look_at': [0, 5, -10]})
    e.call('capture_view', {'width': 960, 'height': 560, 'game_hour': 16}, image=out('mcp', 'street-above.png'))
    # a facade of the game: made to be seen from the street (its alcove, +x), open behind
    new_street(e)
    place_prefabs(e, ['ind-facade-60'], [0, 0.5, 0])
    box = e.call('list_groups')['groups'][-1]['box']
    c = [(box[0][k] + box[1][k]) / 2 for k in range(3)]
    d = max(box[1][k] - box[0][k] for k in range(3)) * 1.1
    e.call('select', {'ids': []})
    for name, at in (('front', [c[0] + d, c[1] + d * 0.2, c[2] + d * 0.15]),
                     ('back', [c[0] - d, c[1] + d * 0.35, c[2] + d * 0.5])):
        e.call('set_camera', {'position': at, 'look_at': c})
        e.call('capture_view', {'width': 720, 'height': 450, 'game_hour': 11}, image=out('mcp', 'facade-%s.png' % name))


SECTIONS = (('getting-started', getting_started), ('levels', levels), ('selecting', selecting), ('groups', groups_),
            ('catalog', catalog), ('prefabs', prefabs), ('building', building), ('holes', holes),
            ('game-rendering', game_rendering), ('behaviors', behaviors), ('export', exporting), ('mcp', assistant))


def main():
    global OUT
    ap = argparse.ArgumentParser()
    ap.add_argument('--port', type=int, default=47896, help="the editor's remote control")
    ap.add_argument('--devtools', type=int, default=9333, help="its web view's DevTools")
    ap.add_argument('--only', default='', help=', '.join(name for name, _ in SECTIONS))
    ap.add_argument('--out', default=OUT, help='where the pictures go (docs/captures)')
    ap.add_argument('--editor', default='', help='the program (by default target/release, else target/debug)')
    args = ap.parse_args()
    OUT = args.out
    only = set(filter(None, args.only.split(',')))
    e = Editor(args.editor or find_editor(), args.port, args.devtools)
    try:
        for name, f in SECTIONS:
            if not only or name in only:
                print(name)
                f(args, e)
    finally:
        e.close()
    print('Not taken by this script (by hand):\n  ' + '\n  '.join(MANUAL))


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Makes the pictures of the documentation (README.md, docs/mcp-agent-guide.md) in
docs/captures/<feature>/. It starts the editor (target/release or target/debug jak-level-editor,
built before) and drives it two ways:

- its remote control (the MCP server's tools: open a level, place, set the camera and the view...)
  sets up each scene, and `capture_view` gives a picture of the 3D view alone;
- the DevTools protocol of its web view (WebView2, Windows) gives a picture of the whole window
  (Page.captureScreenshot, 1600 x 900), clicks a tab, a menu or a button by its English label,
  presses keys, drives the mouse over the view (Input.dispatchMouseEvent: the cursor's label, a box
  being drawn, the button held, a drag, the terrain brush, the topology tool) and makes any call of
  the engine as the interface does.

The editor runs with preferences of its own (<work>/prefs: the library given, English, the guide
already seen), never the user's; the first start's pictures with none at all (<work>/prefs-first).
The paths the windows show are replaced in the pictures (`paths()`): the library is shown as
C:/Users/you/Documents/Jak Level Editor, the folder the editor proposes. What the script writes goes
to <work> (an export, an extraction), never into the library: it only reads it. The remote
control's requests carry the token the editor writes into <work>/prefs/remote.json.

Needs Jak 3 (and Jak II for its pictures) extracted with the editor, the disc it came from for the
extraction's picture (--disc, else the OpenGOAL Launcher's extracted disc), and the prefabs of yours
the pictures use (main-town-building-a to -d, small-fountain, haven-tree, ind-facade-60). From the
repository root:

  python tools/capture_docs.py [--only getting-started,levels,...] [--out <folder>] [--editor <program>]

The pictures show levels of Jak 3 extracted from a copy of the game. Jak 3 belongs to Sony
Interactive Entertainment and was developed by Naughty Dog.
"""

import argparse
import base64
import json
import math
import os
import shutil
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
# where the script writes (preferences, web view data, an export, an extraction): --work
WORK = os.path.join(tempfile.gettempdir(), 'jle-captures')
LIBRARY = os.path.join(REPO, 'library')
DISC = ''  # --disc
# the folder shown in the pictures for the library (the one the editor proposes)
SHOWN_LIBRARY = 'C:/Users/you/Documents/Jak Level Editor'

# Input.dispatchMouseEvent's buttons and modifiers
LEFT, RIGHT = 1, 2
ALT, CTRL, SHIFT = 1, 2, 8


def find_editor():
    for profile in ('release', 'debug'):
        p = os.path.join(REPO, 'target', profile, EXE)
        if os.path.isfile(p):
            return p
    raise SystemExit('build the editor first: npx vite build, then cargo build --release -p jak-level-editor')


def paths():
    """What the pictures show instead of a path of this computer: [(path, shown)], the longest
    first, each with / and with \\."""
    home = os.path.expanduser('~')
    pairs = [(DISC, 'C:/Users/you/jak-level-editor/iso_data/jak3')] if DISC else []
    pairs += [(os.path.join(WORK, 'exports'), SHOWN_LIBRARY + '/exports'), (os.path.join(WORK, 'extract'), SHOWN_LIBRARY),
             (LIBRARY, SHOWN_LIBRARY), (os.path.join(REPO, 'build', 'bin'), 'C:/Users/you/AppData/Local/Jak Level Editor'),
             (REPO, 'C:/Users/you/jak-level-editor'), (WORK, 'C:/Users/you/Documents/Jak Level Editor'),
             (home, 'C:/Users/you')]
    out = []
    for path, shown in pairs:
        path = os.path.normpath(path)
        for sep in ('/', '\\'):
            p = path.replace('\\', sep).replace('/', sep)
            for v in {p, p[:1].lower() + p[1:], p[:1].upper() + p[1:]}:
                out.append((v, shown.replace('/', sep)))
    return sorted(out, key=lambda x: -len(x[0]))


class Editor:
    """The editor started with its remote control on `port` and its web view's DevTools on
    `devtools`, its preferences in `prefs` (prefs.json written from `prefs_json`, or none) and a
    WebView2 data folder of its own (one shared with another running editor would keep its window
    from appearing)."""

    def __init__(self, exe, port, devtools, prefs, prefs_json, wv2):
        os.makedirs(prefs, exist_ok=True)
        self.prefs = prefs
        file = os.path.join(prefs, 'prefs.json')
        if prefs_json is None:
            if os.path.exists(file):
                os.remove(file)
        else:
            with open(file, 'w', encoding='utf-8') as f:
                json.dump(prefs_json, f)
        env = dict(os.environ, WEBVIEW2_USER_DATA_FOLDER=wv2, JLE_PREFS_DIR=prefs,
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

    def token(self):
        """The token of the remote control's requests: its remote.json, written when it listens."""
        for _ in range(50):
            try:
                with open(os.path.join(self.prefs, 'remote.json'), encoding='utf-8') as f:
                    return json.load(f)['token']
            except (OSError, ValueError, KeyError):
                time.sleep(0.1)
        raise SystemExit('the editor wrote no remote.json')

    def call(self, tool, args=None, image=None, check=True):
        """A tool of the remote control (the MCP server's)."""
        self.n += 1
        request = {'id': self.n, 'tool': tool, 'args': args or {}, 'token': self.token()}
        self.f.write((json.dumps(request) + '\n').encode())
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
            write_picture(image, base64.b64decode(a['image']))
        return a.get('result')

    def loaded(self):
        for _ in range(900):
            if not self.call('get_status')['document']['loading']:
                return
            time.sleep(0.2)

    def shot(self, path, settle=1.0, mouse=False):
        """A picture of the whole window, once the view had `settle` seconds to draw; `mouse`: the
        mouse left where it is (a gesture shown), else moved out of the view first."""
        if not mouse:
            self.web.mouse('moved', (SIZE[0] - 4, SIZE[1] - 4))
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

    def click(self, label, within=None, check=True, wait=5.0):
        """The first shown button or entry whose text or aria-label is `label` (the interface in
        English), in the floating window titled `within` if given; waited for `wait` seconds."""
        for _ in range(int(wait / 0.2) + 1):
            if self._click(label, within):
                time.sleep(0.3)
                return True
            time.sleep(0.2)
        if check:
            raise SystemExit('nothing labelled %r to click' % label)
        return False

    def _click(self, label, within):
        return self.evaluate('''(() => {
          const root = %s ? document.querySelector('[role=dialog][aria-label="' + %s + '"]') : document;
          const e = root && [...root.querySelectorAll("button, [role=tab], [role=menuitem], a, label")]
            .find((e) => e.offsetParent && [e.getAttribute("aria-label"), e.textContent.trim()].includes(%s));
          if (e) e.click();
          // its tooltip not shown (they show on focus too)
          document.activeElement && document.activeElement.blur();
          return !!e;
        })()''' % (json.dumps(bool(within)), json.dumps(within or ''), json.dumps(label)))

    def tab(self, name):
        """A panel of the bar of icons on the left: Levels, Build, Catalog, Prefabs, Groups."""
        self.evaluate('document.querySelector(\'nav button[aria-label="%s"]\').click()' % name)
        time.sleep(0.3)

    def menu(self, *path):
        """A menu's item: menu('Level', 'Export for the game...')."""
        for label in path:
            self.click(label)

    def type_into(self, selector, text, focus=False):
        """A text field's value, as typed (its input event); `focus`: left in it."""
        self.evaluate('''(() => {
          const i = document.querySelector(%s);
          i.focus();
          i.value = %s;
          i.dispatchEvent(new Event("input", {bubbles: true}));
          i.dispatchEvent(new Event("change", {bubbles: true}));
          if (!%s) i.blur();
        })()''' % (json.dumps(selector), json.dumps(text), json.dumps(focus)))
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

    def move_window(self, title, to):
        """The floating window `title` dragged by its title bar so that its corner is at `to`."""
        r = self.evaluate('''(() => {
          const h = document.querySelector('[role=dialog][aria-label="%s"] header');
          const r = h.getBoundingClientRect();
          return [r.x, r.y, r.height];
        })()''' % title)
        a = (r[0] + 40, r[1] + r[2] / 2)
        self.drag(a, (to[0] + 40, to[1] + r[2] / 2), steps=10)

    def view(self):
        """The 3D view's box in the page: (x, y, width, height)."""
        r = self.evaluate('(() => { const r = document.querySelector("main canvas").getBoundingClientRect();'
                          ' return [r.x, r.y, r.width, r.height]; })()')
        return tuple(r)

    def center(self, dx=0, dy=0):
        """A point of the view from its middle (where set_camera's look_at is)."""
        x, y, w, h = self.view()
        return (x + w / 2 + dx, y + h / 2 + dy)

    def mouse(self, kind, at, button='none', buttons=0, modifiers=0, clicks=0):
        """One mouse event at `at` (the page's pixels): kind moved, pressed, released."""
        self.send('Input.dispatchMouseEvent', type='mouse' + kind.capitalize(), x=at[0], y=at[1], button=button,
                  buttons=buttons, modifiers=modifiers, clickCount=clicks)

    def drag(self, a, b, button='left', modifiers=0, steps=12, release=True, pause=0.02):
        """The button pressed at `a`, the mouse moved to `b` in steps, let go (or held)."""
        mask = LEFT if button == 'left' else RIGHT
        self.mouse('moved', a, modifiers=modifiers)
        time.sleep(0.1)
        self.mouse('pressed', a, button, mask, modifiers, 1)
        for k in range(1, steps + 1):
            time.sleep(pause)
            self.mouse('moved', (a[0] + (b[0] - a[0]) * k / steps, a[1] + (b[1] - a[1]) * k / steps), button, mask,
                       modifiers)
        if release:
            time.sleep(0.1)
            self.mouse('released', b, button, 0, modifiers, 1)

    def anonymize(self):
        """The paths of this computer the page shows, replaced (`paths()`), until it draws them again."""
        self.evaluate('''((pairs) => {
          const fix = (s) => pairs.reduce((s, [a, b]) => s.split(a).join(b), s);
          const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
          for (let n; (n = w.nextNode()); ) { const v = fix(n.nodeValue); if (v !== n.nodeValue) n.nodeValue = v; }
          document.querySelectorAll("input").forEach((i) => { const v = fix(i.value); if (v !== i.value) i.value = v; });
        })(%s)''' % json.dumps(paths()))

    def shot(self, path):
        self.anonymize()
        os.makedirs(os.path.dirname(path), exist_ok=True)
        png = base64.b64decode(self.send('Page.captureScreenshot', format='png')['data'])
        write_picture(path, png)


def write_picture(path, png):
    """The PNG written, smaller (RGB, the best compression) when Pillow is there; tried again a few
    times: a picture just written may be held a moment (an antivirus, a synchronised folder)."""
    try:
        import io
        from PIL import Image
        with Image.open(io.BytesIO(png)) as im:
            out = io.BytesIO()
            im.convert('RGB').save(out, format='png', optimize=True)
            png = out.getvalue()
    except ImportError:
        pass
    for attempt in range(10):
        try:
            with open(path, 'wb') as f:
                f.write(png)
            return
        except OSError:
            if attempt == 9:
                raise
            time.sleep(0.5)


def out(feature, name):
    return os.path.join(OUT, feature, name)


def open_level(e, name, actors=False, add=()):
    e.call('open_level', {'name': name, 'actors': actors, 'discard_changes': True})
    e.loaded()
    for level in add:
        e.call('add_level', {'name': level})
        e.loaded()
    e.call('select', {'ids': []})
    e.web.tab('Levels')


def camera(e, x, y, z, yaw, pitch):
    e.call('set_camera', {'position': [x, y, z], 'yaw_deg': yaw, 'pitch_deg': pitch})


def frame(e, box, yaw, pitch, k=1.5):
    """The camera looking at the middle of `box` ([min, max]) from yaw / pitch degrees, `k` times its
    size away."""
    c = [(box[0][i] + box[1][i]) / 2 for i in range(3)]
    d = max(box[1][i] - box[0][i] for i in range(3)) * k
    y, p = math.radians(yaw), math.radians(pitch)
    e.call('set_camera', {'position': [c[0] + math.sin(y) * math.cos(p) * d, c[1] - math.sin(p) * d,
                                       c[2] + math.cos(y) * math.cos(p) * d], 'look_at': c})


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
    e.web.tab('Levels')


def getting_started(args, e):
    open_level(e, 'ctyport')
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'few_clouds'})
    camera(e, -150, 140, 1800, -40, -18)
    e.web.tab('Levels')
    e.shot(out('getting-started', 'window.png'), settle=3)
    # the guide at Getting started's fifth step: the move, rotate, scale and snap animations
    e.web.key('F1')
    e.web.click('Tutorials', within='Guide')
    e.web.evaluate('''(() => {
      const g = document.querySelector('[role=dialog][aria-label="Guide"]');
      [...g.querySelectorAll("button")].find((b) => b.textContent.replace(/\\s+/g, " ").trim().startsWith("1.5 ")).click();
    })()''')
    e.shot(out('getting-started', 'guide.png'), settle=2)
    e.web.close_windows()
    # File > Settings: the library, the language, the extractor, the assistant (let in: it already
    # listens for this session)
    e.web.menu('File', 'Settings...')
    e.web.click('Let an assistant control the editor')
    e.shot(out('getting-started', 'settings.png'))
    e.web.close_windows()


def first_start(args, exe):
    """The first start (no preferences: the first-run assistant), then the extraction of the disc into
    <work>/extract (the library is only read)."""
    proposed = os.path.join(os.path.expanduser('~'), 'Documents', 'Jak Level Editor')
    made = not os.path.exists(proposed)
    e = Editor(exe, args.port + 1, args.devtools + 1, os.path.join(WORK, 'prefs-first'), None,
               os.path.join(WORK, 'wv2-first'))
    try:
        e.shot(out('getting-started', 'welcome.png'), settle=2)
        disc = args.disc
        if not disc or not os.path.exists(disc):
            print('  no disc (--disc): extraction.png not taken')
            return
        e.web.key('Escape')
        e.web.close_windows()
        target = os.path.join(WORK, 'extract')
        shutil.rmtree(target, ignore_errors=True)
        os.makedirs(target)
        e.web.menu('File', 'Extract game assets...')
        e.web.type_into('[role=dialog][aria-label="Extract game assets"] input', disc)
        e.web.type_into('[role=dialog][aria-label="Extract game assets"] section:nth-of-type(2) input', target)
        for _ in range(120):
            if (e.web.engine('extract_state') or {}).get('detected', {}).get('game'):
                break
            time.sleep(0.5)
        e.web.click('Extract all the levels')
        for _ in range(1800):
            s = e.web.engine('extract_state') or {}
            if not s.get('running') and (s.get('finished') or s.get('error')):
                break
            time.sleep(1)
        e.shot(out('getting-started', 'extraction.png'), settle=1)
    finally:
        e.close()
        shutil.rmtree(os.path.join(WORK, 'extract'), ignore_errors=True)
        if made and os.path.isdir(proposed) and not os.listdir(proposed):
            os.rmdir(proposed)


def levels(args, e):
    open_level(e, 'ctysluma')
    e.web.tab('Levels')
    e.shot(out('levels', 'levels-panel.png'), settle=3)
    open_level(e, 'ctywide')
    states = e.call('get_status').get('story', {}).get('steps', 0)
    if states:
        e.call('set_story', {'enabled': True, 'step': states})  # the last: after the tower's destruction
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


def street_of_prefabs(e):
    """Two buildings of Main Town and a tree on the paving; returns their groups' ids."""
    new_street(e)
    groups = place_prefabs(e, ['main-town-building-b', 'main-town-building-c'], [-20, 0.5, -10], step=40)
    groups += place_prefabs(e, ['haven-tree'], [45, 0.5, 5])
    e.call('select', {'ids': []})
    e.web.tab('Levels')
    return groups


def selecting(args, e):
    # a building of Spargus selected, the move gizmo on it; a bridge of the industrial zone and the
    # scale gizmo
    open_level(e, 'wascitya')
    house = max(e.call('list_assemblies', {'min_elements': 40, 'limit': 20})['assemblies'], key=lambda a: len(a['ids']))
    e.call('select', {'ids': house['ids']})
    e.call('set_camera', {'frame': house['ids'], 'yaw_deg': 30, 'pitch_deg': -25})
    e.web.key('w')
    e.shot(out('selecting', 'select.png'), settle=3)
    # a catwalk of the industrial zone stretched along its length, the scale gizmo on it
    open_level(e, 'ctyindb')
    walks = [n for n in e.call('find_nodes', {'name': 'catwalk', 'kind': 'render', 'limit': 200})['nodes']
             if 10 < n['size'][0] < 30 and n['size'][1] < 3]
    if walks:
        walk = walks[0]['id']
        e.call('transform_nodes', {'ids': [walk], 'scale_by': [1.8, 1, 1]})
        e.call('select', {'ids': [walk]})
        e.call('set_camera', {'frame': [walk], 'yaw_deg': 35, 'pitch_deg': -25})
        e.web.key('r')
        e.shot(out('selecting', 'scale.png'), settle=3)
        e.web.key('w')
    # the mouse over the view: two buildings of Main Town, a tree
    street_of_prefabs(e)
    e.call('set_camera', {'position': [10, 40, 95], 'look_at': [-20, 12, -10]})
    time.sleep(2)
    # what the cursor points at: outlined, its prefab outlined faintly, the label
    e.web.mouse('moved', e.web.center(-6, 0))
    time.sleep(0.2)
    e.web.mouse('moved', e.web.center())
    e.shot(out('selecting', 'hover.png'), settle=1.5, mouse=True)
    # a box being drawn from the paving, what it would select outlined and counted
    e.web.drag(e.web.center(-230, -120), e.web.center(60, 150), release=False, steps=20)
    e.shot(out('selecting', 'box.png'), settle=0.8, mouse=True)
    e.web.key('Escape')
    e.web.mouse('released', e.web.center(60, 150), 'left', 0, 0, 1)
    e.call('select', {'ids': []})
    # the button held still on the building: the ring filling, then all of the prefab
    at = e.web.center()
    e.web.mouse('moved', at)
    time.sleep(0.2)
    e.web.mouse('pressed', at, 'left', LEFT, 0, 1)
    e.shot(out('selecting', 'hold.png'), settle=0.35, mouse=True)
    time.sleep(0.6)
    e.web.mouse('released', at, 'left', 0, 0, 1)
    # the prefab selected, dragged up and to the right: see-through where it was, how far it went
    time.sleep(0.5)
    to = (at[0] - 150, at[1] - 60)
    e.web.drag(at, to, release=False, steps=20)
    e.shot(out('selecting', 'moving.png'), settle=0.8, mouse=True)
    e.web.key('Escape')
    e.web.mouse('released', to, 'left', 0, 0, 1)
    e.call('select', {'ids': []})


def groups_(args, e):
    new_street(e)
    place_prefabs(e, ['main-town-building-c', 'main-town-building-d'], [-40, 0.5, -10], step=40)
    square = place_prefabs(e, ['small-fountain', 'haven-tree', 'haven-tree'], [-20, 0.5, 35], step=14)
    g = e.call('group', {'ids': [i for g in square for i in g], 'name': 'Little square'})
    e.call('select', {'group': g['group']})
    e.call('set_camera', {'position': [10, 55, 120], 'look_at': [-10, 5, 10]})
    e.web.tab('Groups')
    e.shot(out('groups', 'panel.png'), settle=3)
    # two trees grouped: Ctrl+Shift+G, a name typed
    e.call('ungroup', {'group': g['group']})
    e.call('select', {'ids': square[1] + square[2]})
    e.web.key('G', ctrl=True, shift=True)
    e.web.type_into('[role=dialog][aria-label="New group"] input', 'Two trees', focus=True)
    e.shot(out('groups', 'new-group.png'), settle=0.5)
    e.web.close_windows()
    e.call('select', {'ids': []})


def catalog(args, e):
    open_level(e, 'ctyinda')
    e.call('list_catalog', {'limit': 1})  # the levels read for the catalog
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'few_clouds'})
    b = e.call('get_status')['document']['bounds']
    frame(e, [b['min'], b['max']], 140, -28, 0.7)
    e.web.tab('Catalog')
    for tab, name in (('Objects', 'objects.png'), ('Interactive', 'interactive.png'), ('Effects', 'effects.png'),
                      ('Models', 'models.png'), ('Decor', 'decor-search.png'), ('Materials', 'materials.png')):
        e.web.click(tab)
        if tab == 'Decor':
            e.web.type_into('aside input[placeholder^="Search the decor"]', 'catwalk')
        e.shot(out('catalog', name), settle=4)


def prefabs(args, e):
    open_level(e, 'ctyinda')
    e.web.tab('Prefabs')
    e.shot(out('prefabs', 'panel.png'), settle=5)
    e.web.type_into('aside input[placeholder^="Search"]', 'gate')
    e.shot(out('prefabs', 'search.png'), settle=4)
    e.web.type_into('aside input[placeholder^="Search"]', '')
    # a tower of Spargus selected whole (its assembly), then made a prefab: the small pieces left out
    open_level(e, 'wascitya')
    houses = e.call('list_assemblies', {'min_elements': 40, 'limit': 60})['assemblies']
    house = min(houses, key=lambda a: abs(max(a['box'][1][k] - a['box'][0][k] for k in (0, 2)) - 35))
    e.call('select', {'assembly': house['ids'][0]})
    frame(e, house['box'], 30, -32, 1.4)
    e.shot(out('prefabs', 'assembly.png'), settle=3)
    e.web.click('Create a prefab')
    e.shot(out('prefabs', 'create.png'), settle=1)
    e.web.close_windows()


def building(args, e):
    # a yard of the port: a floor and walls in the style of ctyport, one of them bent
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('set_view', {'game_view': False})
    style = {'kind': 'shape', 'style': 'ctyport'}
    e.call('place', dict(style, name='ground', position=[0, 0, 0], size=[24, 0.5, 24]))
    e.call('place', dict(style, name='wall', position=[0, 0, -12], size=[24, 6, 0.5]))
    e.call('place', dict(style, name='wall', position=[-12, 0, 0], size=[24, 6, 0.5], yaw_deg=90, bend=-60))
    e.call('place', dict(style, name='pillar', position=[-4, 0, 6], size=[2, 5, 2], round=1))
    e.call('place', dict(style, name='stairs', position=[4, 0.5, 2], size=[3, 4, 9], steps=12, bend=120, yaw_deg=180))
    e.call('select', {'ids': []})
    e.web.engine('level_style', {'level': 'ctyport'})
    e.call('set_camera', {'position': [20, 18, 26], 'look_at': [0, 1, -2]})
    e.web.tab('Build')
    e.shot(out('building', 'shapes.png'), settle=3)
    # the Build panel at its level style: what it measured of the port
    e.web.evaluate("""(() => {
      const h = [...document.querySelectorAll("aside h3")].find((h) => h.textContent.trim() === "In the style of a level");
      h && h.scrollIntoView({block: "start"});
    })()""")
    e.call('set_camera', {'position': [14, 9, 17], 'look_at': [0, 1.5, -1]})
    e.shot(out('building', 'style.png'), settle=3)
    terrain(args, e)
    topology(args, e)
    preview(args, e)
    # the atoll, a level of Jak II, opened in the game rendering
    if 'error' not in e.call('list_levels', {'game': 'jak2'}, check=False):
        e.call('open_level', {'name': 'atoll', 'game': 'jak2', 'actors': False, 'discard_changes': True})
        e.loaded()
        e.call('set_view', {'game_view': True, 'hour': 11, 'weather': 'few_clouds'}, check=False)
        e.call('set_camera', {'all': True})
        e.shot(out('building', 'jak2-level.png'), settle=4)


def brush(e, **settings):
    """The terrain brush's settings (its window's)."""
    e.web.engine('set_brush', settings)


def terrain(args, e):
    """The terrain brush: hills of the desert shaped, the right button raising one, the magnet brush.
    The editor's own view (the sand as it is, without a level's light)."""
    # the magnet brush next to a terrain laid 3 m up: the square at its level, then held
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('set_view', {'game_view': False})
    t = e.call('terrain', {'mode': 'paint', 'position': [-8, 3, 0], 'to': [8, 3, 0], 'size': 10, 'style': 'deserta',
                           'height': 3, 'relief': False})
    e.call('set_camera', {'position': [-10, 20, 34], 'look_at': [6, 2, 0]})
    e.call('select', {'ids': [t['id']]})
    e.web.tab('Build')
    e.web.key('b')
    brush(e, mode='paint', magnet=True, size=6)
    e.call('select', {'ids': []})
    time.sleep(1)
    # beside its far edge (the brush's window covers the right of the view)
    side = e.web.center(-120, -40)
    for k in range(6):
        e.web.mouse('moved', (side[0] - 30 + k * 6, side[1]))
        time.sleep(0.1)
    e.shot(out('building', 'terrain-magnet.png'), settle=1, mouse=True)
    to = (side[0] - 150, side[1] + 60)
    e.web.drag(side, to, release=False, steps=16)
    e.shot(out('building', 'terrain-magnet-held.png'), settle=0.8, mouse=True)
    e.web.key('Escape')
    e.web.mouse('released', to, 'left', 0, 0, 1)
    brush(e, magnet=False)
    e.web.key('b')
    # hills of the desert: a riverbed dug, a yard flattened and walled, a hill raised
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('set_view', {'game_view': False})
    t = e.call('terrain', {'mode': 'generate', 'position': [0, 0, 0], 'area': [96, 96], 'hills': 22, 'roughness': 0.55,
                           'style': 'deserta'})
    tid = t['id']
    for stroke in ({'mode': 'dig', 'position': [-45, 0, 22], 'to': [45, 0, 12], 'size': 10, 'amount': 6},
                   {'mode': 'flatten', 'position': [18, 8, -24], 'size': 22, 'height': 8, 'amount': 1},
                   {'mode': 'wall', 'position': [9, 8, -33], 'to': [28, 8, -33], 'size': 2, 'amount': 4},
                   {'mode': 'wall', 'position': [28, 8, -33], 'to': [28, 8, -15], 'size': 2, 'amount': 4},
                   {'mode': 'raise', 'position': [-28, 0, -26], 'size': 22, 'amount': 14}):
        e.call('terrain', dict(stroke, id=tid))
    e.call('select', {'ids': [tid]})
    e.call('set_camera', {'position': [-30, 55, 100], 'look_at': [8, 0, -10]})
    e.web.tab('Build')
    e.web.key('b')
    brush(e, mode='paint', magnet=False, size=8)
    e.call('select', {'ids': []})
    time.sleep(1.5)
    # the brush's square over the hills
    e.web.mouse('moved', e.web.center(-120, 70))
    e.web.mouse('moved', e.web.center(-110, 80))
    e.shot(out('building', 'terrain.png'), settle=1, mouse=True)
    # the right button held, the mouse moved up: the hill it raises, before it is let go
    brush(e, size=14)
    at = e.web.center(-110, 80)
    e.web.drag(at, (at[0], at[1] - 110), button='right', release=False, steps=16)
    e.shot(out('building', 'terrain-raise.png'), settle=0.8, mouse=True)
    e.web.key('Escape')
    e.web.mouse('released', (at[0], at[1] - 110), 'right', 0, 0, 1)
    e.web.key('b')


def topology(args, e):
    """A floor whose faces were cut in four, half of them picked and pulled up 1 m: a step."""
    e.call('new_level', {'discard_changes': True})
    e.loaded()
    e.call('set_view', {'game_view': False})
    f = e.call('place', {'kind': 'shape', 'name': 'floor', 'position': [0, 0, 0], 'size': [8, 0.5, 8]})
    e.call('select', {'ids': [f['nodes'][0]['id']]})
    e.web.key('t')
    time.sleep(0.5)
    # the faces cut in four (as Faces, All, Subdivide do), none picked
    e.web.engine('topology_select', {'faces': True, 'all': True})
    e.web.engine('topology_subdivide')
    e.web.engine('topology_select', {'all': False})
    # a box over the top faces of the left half: seen from the side at the floor's middle, the top
    # faces' centers are above the middle of the view, the bottom ones under it
    e.call('set_camera', {'position': [0, 0.25, 20], 'look_at': [0, 0.25, 0]})
    time.sleep(1)
    x, y, w, h = e.web.view()
    e.web.drag((x + 2, y + 2), e.web.center(-2, -2), steps=8)
    e.web.engine('topology_extrude', {'distance': 1})
    e.call('set_camera', {'position': [8, 7, 10], 'look_at': [0, 0.5, 0]})
    e.shot(out('building', 'topology.png'), settle=1)
    e.web.key('t')


def preview(args, e):
    """A breakable fence of the factory thrown apart (the inspector's Preview, Break it)."""
    new_street(e)
    e.call('set_view', {'game_view': False})
    f = e.call('place', {'kind': 'object', 'name': 'fac-break-fence-a', 'position': [0, 0.5, 0], 'on_ground': True})
    fid = f['nodes'][0]['id']
    e.call('select', {'ids': [fid]})
    frame(e, f['nodes'][0]['box'], 35, -18, 1.3)
    time.sleep(3)
    e.web.click('Break it')
    e.shot(out('building', 'preview.png'), settle=1.2)
    e.web.click('Stop', check=False, wait=0)


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
    print('  gaps:', {k: r.get(k) for k in ('gaps', 'edges', 'viewpoints')})
    e.call('select', {'ids': []})
    e.web.tab('Levels')
    e.call('set_camera', {'position': [30, 26, 40], 'look_at': [0, 0, 0]})
    x, y, w, h = e.web.view()
    e.web.move_window('Holes in the decor', (x + 8, y + 8))
    e.shot(out('holes', 'window.png'), settle=3)
    e.call('capture_view', {'width': 960, 'height': 560}, image=out('holes', 'assistant-view.png'))
    e.web.close_windows()


def game_rendering(args, e):
    open_level(e, 'ctysluma')
    status = e.call('get_status')['document']
    lo, hi = status['bounds']['min'], status['bounds']['max']
    mid = [(lo[k] + hi[k]) / 2 for k in range(3)]
    e.call('set_camera', {'position': [mid[0], hi[1] + 25, hi[2] + 40], 'look_at': [mid[0], hi[1] - 10, mid[2]]})
    e.call('set_view', {'game_view': True, 'hour': 12, 'weather': 'clear'})
    e.web.menu('View', 'Sky, hour and weather...')
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
    e.web.close_windows()
    # exported into <work>/exports (shown as the library's exports/): its report
    folder = os.path.join(WORK, 'exports', 'my-ctyslum')
    shutil.rmtree(os.path.join(WORK, 'exports'), ignore_errors=True)
    e.web.menu('Level', 'Export for the game...')
    win = '[role=dialog][aria-label="Export for the game"]'
    e.web.type_into(win + ' input', 'my-ctyslum')
    e.web.evaluate('''(() => {
      const i = [...document.querySelectorAll('%s input')].find((i) => i.parentElement.textContent.includes("Written in"));
      i.value = %s; i.dispatchEvent(new Event("input", {bubbles: true}));
    })()''' % (win, json.dumps(folder)))
    e.web.click('Export', within='Export for the game')
    for _ in range(600):
        if e.web.evaluate('!!document.querySelector(\'%s\') && document.querySelector(\'%s\').textContent.includes("Exported")'
                          % (win, win)):
            break
        time.sleep(0.5)
    e.shot(out('export', 'export-window.png'), settle=1)
    # no game to play in (the standalone app): the end of its report, the guide's tutorial on adding
    # the level by hand offered
    e.web.evaluate('''(() => {
      const w = document.querySelector('%s');
      for (const el of w.querySelectorAll("*")) if (el.scrollHeight > el.clientHeight + 4) el.scrollTop = el.scrollHeight;
    })()''' % win)
    e.shot(out('export', 'by-hand.png'), settle=1)
    e.web.close_windows()
    shutil.rmtree(os.path.join(WORK, 'exports'), ignore_errors=True)
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
    global OUT, WORK, LIBRARY, DISC
    ap = argparse.ArgumentParser()
    ap.add_argument('--port', type=int, default=47896, help="the editor's remote control (the first start's: +1)")
    ap.add_argument('--devtools', type=int, default=9333, help="its web view's DevTools (the first start's: +1)")
    ap.add_argument('--only', default='', help=', '.join(['first-start'] + [name for name, _ in SECTIONS]))
    ap.add_argument('--out', default=OUT, help='where the pictures go (docs/captures)')
    ap.add_argument('--editor', default='', help='the program (by default target/release, else target/debug)')
    ap.add_argument('--library', default=LIBRARY, help='the library to read (library/ of the repository)')
    ap.add_argument('--work', default=WORK, help='where the script writes (preferences, an export, an extraction)')
    ap.add_argument('--disc', default='', help="the Jak 3 disc folder or .iso the extraction's picture reads")
    args = ap.parse_args()
    OUT, WORK, LIBRARY = args.out, os.path.abspath(args.work), os.path.abspath(args.library)
    if not args.disc:
        # the OpenGOAL Launcher's extracted disc, if it is there
        try:
            with open(os.path.join(os.environ.get('APPDATA', ''), 'OpenGOAL-Launcher', 'settings.json')) as f:
                install = json.load(f).get('installationDir') or ''
            args.disc = os.path.join(install, 'active', 'jak3', 'data', 'iso_data', 'jak3')
        except (OSError, ValueError):
            pass
    DISC = os.path.normpath(args.disc) if args.disc else ''
    only = set(filter(None, args.only.split(',')))
    exe = os.path.abspath(args.editor) if args.editor else find_editor()
    if not only or 'first-start' in only:
        print('first-start')
        first_start(args, exe)
    if only and only <= {'first-start'}:
        return
    e = Editor(exe, args.port, args.devtools, os.path.join(WORK, 'prefs'),
               {'library_root': LIBRARY.replace('\\', '/'), 'library_chosen': True, 'guide_seen': True, 'language': 'en'},
               os.path.join(WORK, 'wv2'))
    try:
        for name, f in SECTIONS:
            if not only or name in only:
                print(name)
                f(args, e)
    finally:
        e.close()


if __name__ == '__main__':
    main()

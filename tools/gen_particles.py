#!/usr/bin/env python3
"""Writes data/jak3/particles.json: the particle effects of Jak 3, read in the decompiled game code
of a jak-project checkout (goal_src/jak3, read only). The editor draws the effects of the levels
(the part-spawner actors and their kind name a particle group by its art-name) with them.

  groups     per group name ("group-ctyindb-fast-steam"): its id, flags, bounds (meters) and its
             parts, each a launcher by id with when it launches (sp-item: flags, period, length,
             offset in the game's ticks, 300 a second; fade-after, falloff-to in meters; binding)
  launchers  per id: its init specs, in the order of the game's fields, each
             [field, kind, a, b, c]:
               f     a float: a + c * rand() * b
               fi    a float with an integer random: a + c * int(rand() * (b + 1))
               i     an integer: a + c * int(rand() * b)
               copy  the value of the field b (a field before it)
               sym   a symbol (a function the game calls: b)
               next  the launcher b (next-launcher)
               flags the flags b (a list of names)
               tex   the texture b: "<tpage>/<name>", or "<page>:<index>" when the code names none
             Values are the game's units: meters times 4096, angles in 65536 a turn, times in
             ticks (300 a second).
  textures   the texture pages and textures the launchers draw

It also writes extractor/data/jak3/sprite-pages.json: the texture pages ogle-extract writes to
library/jak3/common/sprites/ for the editor to draw the particles with.

Usage: python tools/gen_particles.py <jak-project>/goal_src
"""

import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)

# sp-field-id (sparticle-launcher-h.gc): the init specs' fields, in order
FIELDS = {
    'texture': 1, 'anim': 2, 'anim-speed': 3, 'birth-func': 4, 'joint/refpoint': 5, 'num': 6, 'sound': 7,
    'x': 10, 'y': 11, 'z': 12, 'scale-x': 13, 'rot-x': 14, 'rot-y': 15, 'rot-z': 16, 'scale-y': 17,
    'r': 18, 'g': 19, 'b': 20, 'a': 21,
    'omega': 24, 'vel-x': 25, 'vel-y': 26, 'vel-z': 27, 'scalevel-x': 28, 'rotvel-x': 29, 'rotvel-y': 30,
    'rotvel-z': 31, 'scalevel-y': 32, 'fade-r': 33, 'fade-g': 34, 'fade-b': 35, 'fade-a': 36,
    'accel-x': 37, 'accel-y': 38, 'accel-z': 39, 'dummy': 40, 'quat-x': 41, 'quat-y': 42, 'quat-z': 43,
    'quad-w': 44, 'friction': 45, 'timer': 46, 'flags': 47, 'userdata': 48, 'func': 49, 'next-time': 50,
    'next-launcher': 51,
    'launchrot-x': 54, 'launchrot-y': 55, 'launchrot-z': 56, 'launchrot-w': 57, 'conerot-x': 58,
    'conerot-y': 59, 'conerot-z': 60, 'conerot-w': 61, 'rotate-x': 62, 'rotate-y': 63, 'rotate-z': 64,
    'conerot-radius': 65, 'mat-scale-x': 66, 'mat-scale-y': 67, 'mat-scale-z': 68,
    'scale': 70, 'scalevel': 71,
}


# --- a small reader of GOAL forms -------------------------------------------------------------

TOKEN = re.compile(r'''\s*(?:(;[^\n]*)|("(?:\\.|[^"\\])*")|(#\|[\s\S]*?\|#)|([()'])|([^\s()';"]+))''')


def read_forms(text):
    """The top-level forms of a GOAL file: lists, symbols (str), numbers, strings ("..." kept)."""
    stack = [[]]
    quote = []
    pos = 0
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m or m.end() == pos:
            break
        pos = m.end()
        comment, string, block, punct, atom = m.groups()
        if comment or block:
            continue
        if punct == '(':
            stack.append([])
            quote.append(None)
            continue
        if punct == ')':
            if len(stack) == 1:
                continue
            done = stack.pop()
            quote.pop()
            item = done
        elif punct == "'":
            stack.append(['quote'])
            quote.append('q')
            continue
        else:
            item = string if string is not None else atom
        stack[-1].append(item)
        # a quote takes one form
        while quote and quote[-1] == 'q' and len(stack[-1]) == 2 and stack[-1][0] == 'quote':
            q = stack.pop()
            quote.pop()
            stack[-1].append(q)
    return stack[0]


def number(x):
    """A number of GOAL source (or None): 1, 1.5, #x10, #b101, -3."""
    if not isinstance(x, str):
        return None
    try:
        if x.startswith('#x'):
            return int(x[2:], 16)
        if x.startswith('#b'):
            return int(x[2:], 2)
        if re.fullmatch(r'-?\d+', x):
            return int(x)
        return float(x)
    except ValueError:
        return None


def value(x):
    """A value of an init spec in the game's units, or None: numbers, (meters n), (degrees n),
    (seconds n), and a few operators."""
    n = number(x)
    if n is not None:
        return n
    if isinstance(x, list) and x:
        head, args = x[0], [value(a) for a in x[1:]]
        if any(a is None for a in args):
            return None
        if head == 'meters' and len(args) == 1:
            return args[0] * 4096.0
        if head == 'degrees' and len(args) == 1:
            return args[0] * 65536.0 / 360.0
        if head == 'seconds' and len(args) == 1:
            return int(round(args[0] * 300))
        if head == '/' and len(args) == 2 and args[1]:
            return args[0] / args[1]
        if head == '*' and args:
            r = 1.0
            for a in args:
                r *= a
            return r
        if head == '+' and args:
            return sum(args)
        if head == '-' and len(args) == 1:
            return -args[0]
        if head == '-' and len(args) == 2:
            return args[0] - args[1]
    return None


def is_int_param(x):
    return (number(x) is not None and isinstance(number(x), int)) or (isinstance(x, list) and x and x[0] == 'seconds')


def keywords(items):
    """(:key value ...) as a dict"""
    out = {}
    i = 0
    while i < len(items):
        if isinstance(items[i], str) and items[i].startswith(':') and i + 1 < len(items):
            out[items[i][1:]] = items[i + 1]
            i += 2
        else:
            i += 1
    return out


def texture(spec):
    """(name tpage) or (new 'static 'texture-id :index n :page n)"""
    if isinstance(spec, list) and len(spec) == 2 and all(isinstance(s, str) for s in spec):
        return spec[1] + '/' + spec[0]
    if isinstance(spec, list) and spec and spec[0] == 'new':
        k = keywords(spec[3:])
        return '%d:%d' % (number(k.get('page', '0')) or 0, number(k.get('index', '0')) or 0)
    return None


def init_spec(spec):
    """One (:field params...) as [field, kind, a, b, c], or None"""
    if not isinstance(spec, list) or not spec or not isinstance(spec[0], str) or not spec[0].startswith(':'):
        return None
    name = spec[0][1:]
    field = FIELDS.get(name)
    if field is None:
        return None
    params = [p for p in spec[1:] if p != ':store']
    if name == 'flags':
        flags = params[0] if params and isinstance(params[0], list) else []
        return [field, 'flags', 0, [f for f in flags if isinstance(f, str)], 0]
    if name == 'texture':
        t = texture(params[0]) if params else None
        return [field, 'tex', 0, t, 0] if t else None
    if name == 'next-launcher':
        n = value(params[0]) if params else None
        return [field, 'next', 0, int(n), 0] if n is not None else None
    if len(params) == 2 and params[0] == ':copy':
        other = FIELDS.get(params[1])
        return [field, 'copy', 0, other, 0] if other is not None else None
    if len(params) == 2 and params[0] == ':data':
        return None
    if len(params) == 1 and isinstance(params[0], list) and params[0] and params[0][0] == 'quote':
        return [field, 'sym', 0, params[0][1] if len(params[0]) > 1 else '', 0]
    vals = [value(p) for p in params]
    if not vals or any(v is None for v in vals):
        return None
    floats = [isinstance(v, float) for v in vals]
    if len(vals) == 1:
        return [field, 'f' if floats[0] else 'i', vals[0], 0, 1]
    if len(vals) == 2:
        if not floats[0] and not floats[1]:
            return [field, 'i', vals[0], vals[1], 1]
        return [field, 'f', float(vals[0]), float(vals[1]), 1.0]
    if len(vals) == 3:
        if is_int_param(params[1]) and floats[0]:
            return [field, 'fi', vals[0], int(vals[1]), vals[2]]
        if not any(floats):
            return [field, 'i', vals[0], vals[1], vals[2]]
        return [field, 'f', float(vals[0]), float(vals[1]), float(vals[2])]
    return None


def sp_item(form):
    """(sp-item launcher :key value...) as [launcher, flags, period, length, offset, fade-after,
    falloff-to, binding, hour-mask]"""
    if not isinstance(form, list) or len(form) < 2 or form[0] != 'sp-item':
        return None
    launcher = value(form[1])
    if launcher is None:
        return None
    k = keywords(form[2:])
    flags = k.get('flags', [])

    def v(key, dflt=0):
        x = value(k[key]) if key in k else None
        return dflt if x is None else x

    return [int(launcher), [f for f in flags if isinstance(f, str)] if isinstance(flags, list) else [],
            int(v('period')), int(v('length')), int(v('offset')), v('fade-after') / 4096.0,
            v('falloff-to') / 4096.0, int(v('binding')), int(v('hour-mask'))]


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    src = os.path.join(sys.argv[1], 'jak3')
    groups, launchers, pages = {}, {}, {}
    for path in sorted(glob.glob(os.path.join(src, '**', '*.gc'), recursive=True)):
        text = open(path, encoding='utf-8', errors='replace').read()
        if 'defpart' not in text:
            continue
        for form in read_forms(text):
            if not isinstance(form, list) or not form:
                continue
            if form[0] == 'defpartgroup' and len(form) > 2:
                k = keywords(form[2:])
                parts = [p for p in (sp_item(x) for x in (k.get('parts') or [])) if p]
                bounds = k.get('bounds')
                b = [value(x) for x in bounds[1:5]] if isinstance(bounds, list) and len(bounds) == 5 else None
                rotate = k.get('rotate') if isinstance(k.get('rotate'), list) else None
                scale = k.get('scale') if isinstance(k.get('scale'), list) else None
                groups[form[1]] = {
                    'id': value(k.get('id')),
                    'flags': [f for f in (k.get('flags') or []) if isinstance(f, str)],
                    'bounds': [round(x, 3) for x in b] if b and None not in b else None,
                    'rotate': [value(x) for x in rotate] if rotate else None,
                    'scale': [value(x) for x in scale] if scale else None,
                    'parts': parts,
                }
            elif form[0] == 'defpart' and len(form) > 1:
                pid = value(form[1])
                k = keywords(form[2:])
                specs = [s for s in (init_spec(x) for x in (k.get('init-specs') or [])) if s]
                if pid is not None:
                    launchers[str(int(pid))] = specs
                    for s in specs:
                        if s[1] == 'tex' and '/' in s[3]:
                            page, name = s[3].split('/', 1)
                            pages.setdefault(page, set()).add(name)
    out = {
        'format': 1,
        'game': 'jak3',
        'source': 'goal_src/jak3 (OpenGOAL decompiled code): defpartgroup, defpart',
        'groups': groups,
        'launchers': launchers,
        'textures': {p: sorted(n) for p, n in sorted(pages.items())},
    }
    dst = os.path.join(REPO, 'data', 'jak3', 'particles.json')
    with open(dst, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(out, f, separators=(',', ':'), sort_keys=True)
        f.write('\n')
    print('%s: %d groups, %d launchers, %d texture pages' % (dst, len(groups), len(launchers), len(pages)))
    # the pages ogle-extract writes the textures of (library/jak3/common/sprites/)
    ids = sorted({int(s[3].split(':')[0]) for l in launchers.values() for s in l if s[1] == 'tex' and ':' in s[3]})
    sprite_pages = os.path.join(REPO, 'extractor', 'data', 'jak3', 'sprite-pages.json')
    with open(sprite_pages, 'w', encoding='utf-8', newline='\n') as f:
        json.dump({'pages': sorted(pages), 'page_ids': ids}, f, indent=1)
        f.write('\n')
    print('%s: %d pages, %d by id' % (sprite_pages, len(pages), len(ids)))


if __name__ == '__main__':
    main()

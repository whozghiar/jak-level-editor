#!/usr/bin/env python3
"""Writes data/jak2/creatures.json: the actor types of Jak II that are living things (enemies, the
people of the city, the sidekicks), from Jak II's type table (a jak-project checkout's
decompiler/config/jak2/all-types.gc, or an OpenGOAL installation's), as tools/gen_game_data.py finds
Jak 3's: a type deriving from one of CREATURES. The catalog leaves their models out of Jak II's
objects (the editor places no creature).

Usage: python tools/gen_jak2_creatures.py <jak-project>/decompiler/config/jak2/all-types.gc
"""

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
CREATURES = ['enemy', 'process-taskable', 'mech', 'sidekick', 'target', 'bot', 'citizen']
# the bosses and the living things of the cutscenes, which derive from no type of CREATURES
MORE = ['babak', 'big-bopper', 'dig-balloon-lurker', 'errol-racer', 'intro-flamer', 'metalkor', 'minnow',
        'particleman', 'roboguard', 'son-of-particleman', 'spider-eyes', 'squid', 'tomb-boulder-spider', 'widow',
        'wren']


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return
    with open(sys.argv[1], encoding='utf-8', errors='replace') as f:
        text = f.read()
    parent = {m.group(1): m.group(2) for m in re.finditer(r'^\(deftype ([^\s()]+)\s+\(([^\s()]+)\)', text, re.M)}

    def lineage(t):
        out = [t]
        while t in parent and len(out) < 64:
            t = parent[t]
            out.append(t)
        return out

    creatures = sorted({t for t in parent if 'process' in lineage(t) and any(x in CREATURES for x in lineage(t))} | set(MORE))
    path = os.path.join(REPO, 'data', 'jak2', 'creatures.json')
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        json.dump({'_about': 'The actor types of Jak II that are living things, from its type table: written by '
                             'tools/gen_jak2_creatures.py. The catalog leaves their models out.',
                   'creatures': creatures}, f, indent=1)
        f.write('\n')
    print('%d creature types' % len(creatures))


if __name__ == '__main__':
    main()

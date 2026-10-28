#!/usr/bin/env python3
"""Writes data/jak2/texture-slots.json and data/jak3/texture-slots.json: the animated texture slots
of OpenGOAL's renderer, read in common/texture/texture_slots.cpp of a jak-project checkout (read
only; the release the extraction uses). A model's draw in a level's .fr3 names its animated
texture by slot (a negative texture id, -(slot + 1)); the extraction (io/extraction/models.rs)
gives that draw the slot's texture, as OpenGOAL's own model files do.

Usage: python tools/gen_texture_slots.py <jak-project>/common/texture/texture_slots.cpp
"""

import json
import os
import re
import sys


def main():
    text = open(sys.argv[1], encoding="utf-8").read()
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")
    for game in ["jak2", "jak3"]:
        body = re.search(r"std::vector<std::string> %s_slots = \{(.*?)\};" % game, text, re.S).group(1)
        # the names in order, the commented ones left out
        slots = re.findall(r'^\s*"([^"]+)",', body, re.M)
        path = os.path.join(root, game, "texture-slots.json")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            json.dump({"slots": slots}, f, indent=1)
            f.write("\n")
        print(path, len(slots))


if __name__ == "__main__":
    main()

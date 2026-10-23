// The games of the series. The editor edits Jak 3's levels and exports them for Jak 3; the other
// games are planned: the interface shows them marked "Work in progress" (the class `wip`), and
// they cannot be chosen. One becomes `ready` once the engine reads, shows and exports its levels.
// `extractable`: its disc is extracted into <library>/<id>/ (Jak II's for its decor in Jak 3's
// levels today; the same files its own editing will read).
export const games = [
  { id: "jak1", name: "Jak and Daxter", ready: false, extractable: false },
  { id: "jak2", name: "Jak II", ready: false, extractable: true },
  { id: "jak3", name: "Jak 3", ready: true, extractable: true },
] as const;

// the game whose levels the editor edits and exports
export const editedGame = games[2];

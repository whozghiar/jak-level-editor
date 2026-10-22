// The games of the series. The editor edits Jak 3's levels and exports them for Jak 3; the other
// games are planned: the interface shows them marked "Work in progress" (the class `wip`), and
// they cannot be chosen. One becomes `ready` once the engine reads, shows and exports its levels.
export const games = [
  { id: "jak1", name: "Jak and Daxter", ready: false },
  { id: "jak2", name: "Jak II", ready: false },
  { id: "jak3", name: "Jak 3", ready: true },
] as const;

// the game whose levels the editor edits and exports
export const editedGame = games[2];

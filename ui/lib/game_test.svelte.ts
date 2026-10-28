// Level > Try in the game (F5, the toolbar's Try): the level played in the game the user installed
// (crates/core/src/editor/game_test.rs). Its state (game_test_state), read again when a try moves
// on (jle:progress of the job game_test), shared by the toolbar and the menu.

import { t } from "./i18n";
import { call, onEngine } from "./rpc";

export type GameTestState = {
  /** a try can start; else `reason` says why (a key of lang/*.json) */
  available: boolean;
  reason: string;
  busy: boolean;
  /** the game of the last try is open */
  running: boolean;
  last: {
    step?: string;
    error?: string;
    seconds?: number;
    done?: boolean;
  } | null;
};

export const gameTest = $state({
  state: undefined as GameTestState | undefined,
});

export async function refreshGameTest(): Promise<void> {
  gameTest.state =
    (await call<GameTestState>("game_test_state", {}, true)) ?? gameTest.state;
}

export async function testInGame(): Promise<void> {
  const s = gameTest.state;
  if (s && (!s.available || s.busy)) return;
  await call("test_in_game");
  await refreshGameTest();
}

/** What the toolbar's button and the menu's item say when hovered */
export function gameTestTip(): string {
  const s = gameTest.state;
  if (!s) return t("game_test.tip");
  if (s.busy) return s.last?.step ? t(s.last.step) : t("game_test.title");
  if (!s.available) return t(s.reason);
  return t("game_test.tip");
}

/** Follows the tries: their progress, the document opened or closed (the caller's effect) */
export function watchGameTest(): () => void {
  refreshGameTest();
  return onEngine<{ id?: string }>("jle:progress", (p) => {
    if (p?.id === "game_test") refreshGameTest();
  });
}

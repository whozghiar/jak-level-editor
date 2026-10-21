// The calls of a tool of the view, one at a time and in order (a press before its drags, the
// release after them): the moves of the mouse waiting while one is out merged into the last one
// (`merge` gives its arguments; dt summed for a drag).

import { call } from "./engine";

export function callQueue(
  onAnswer: (name: string, answer: any) => void,
  merged: string[],
) {
  const queue: { name: string; args: Record<string, unknown> }[] = [];
  let running = false;
  let idle: (() => void)[] = [];
  async function pump() {
    if (running) return;
    running = true;
    while (queue.length) {
      const q = queue.shift()!;
      try {
        onAnswer(q.name, await call(q.name, q.args));
      } catch (e) {
        console.warn(`[viewport] ${q.name}:`, e);
      }
    }
    running = false;
    for (const f of idle.splice(0)) f();
  }
  return {
    send(name: string, args: Record<string, unknown> = {}) {
      const last = queue[queue.length - 1];
      if (last && last.name === name && merged.includes(name)) {
        if (typeof args.dt === "number")
          args.dt = args.dt + ((last.args.dt as number) ?? 0);
        last.args = args;
      } else {
        queue.push({ name, args });
      }
      void pump();
    },
    /** resolves once every call sent is answered */
    done(): Promise<void> {
      return running || queue.length
        ? new Promise((r) => idle.push(r))
        : Promise.resolve();
    },
  };
}

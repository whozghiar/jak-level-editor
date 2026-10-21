<script lang="ts">
  // The inspector's sections of an actor (behavior_panel.cpp): its behavior, set in simple terms for
  // the objects whose behavior is the game's code (doors, bouncers, elevators, whether an object is
  // there at all), and its parameters, every res lump the code of its type reads (game data: params),
  // typed. The engine's calls: actor_behavior, set_actor_behavior, set_actor_param (behaviors.rs).
  import { t } from "../../lib/i18n";
  import Tip from "../common/Tip.svelte";
  import Condition from "./Condition.svelte";
  import DragNumber from "./DragNumber.svelte";
  import {
    check,
    commit,
    field,
    heading,
    live,
    muted,
    query,
    small,
  } from "./edit.svelte";

  type Enum = { bitfield: boolean; values: [string, number][] };
  type Param = {
    tag: string;
    kind: string;
    value: any;
    set: boolean;
    meters: boolean;
    enum: Enum | null;
  };
  type Behavior = {
    etype: string;
    kind: "door" | "bouncer" | "elevator" | "other";
    behavior: Record<string, any>;
    bouncer_height: number;
    elevator_speed: number;
    crate_type: string | null;
    params: Param[];
    tasks: string[];
    actors: { id: number; name: string; aid?: number }[];
  };

  let { id }: { id: number } = $props();
  const q = query<Behavior>("actor_behavior", () => ({ id }));
  const b = $derived(q.value);

  const kUnits = 4096; // game units per meter
  const doorModes = ["approach", "locked", "always", "when"];
  const crateLooks = ["wood", "iron", "steel", "metal", "blue", "darkeco"];

  const setBehavior = (changes: Record<string, unknown>) =>
    commit("set_actor_behavior", { id, behavior: changes });
  // a lump the behavior section sets (its type given: it may not be among the parameters)
  const lumpArgs = (tag: string, value: unknown) => ({
    id,
    tag,
    value,
    type: "float",
    label: "undo.behavior",
  });
  const setParam = (tag: string, value: unknown) =>
    commit("set_actor_param", { id, tag, value });

  // a lump value as numbers (a number, or an array of them)
  const numbersOf = (v: any): number[] =>
    typeof v === "number"
      ? [v]
      : Array.isArray(v)
        ? v.filter((x) => typeof x === "number")
        : [];
  // C's %g
  const g = (d: number) => String(Number(d.toPrecision(6)));
  const bit = (i: number, k: number) => Math.floor(i / 2 ** k) % 2 === 1;

  function list(p: Param, text: string) {
    // a list of numbers, separated by spaces
    const arr = text
      .split(/[\s,]+/)
      .filter(Boolean)
      .map(Number)
      .filter(Number.isFinite)
      .map((d) => (p.kind === "ints" ? Math.round(d) : d));
    setParam(p.tag, arr);
  }

  function actorName(p: Param): string {
    // another actor of the level, by its name (the game's own links are its ids)
    if (typeof p.value === "string") return p.value;
    const n = numbersOf(p.value);
    if (!n.length) return "";
    return (
      b?.actors.find((a) => a.aid === n[0])?.name ?? t("params.actor.missing")
    );
  }

  const label =
    "w-32 shrink-0 text-xs leading-tight break-words text-neutral-400";
</script>

{#if b}
  {@const beh = b.behavior}
  <h3 class={heading}>{t("behavior.title")}</h3>
  <div class="flex flex-col gap-1.5 text-sm">
    {#if b.kind === "door"}
      {@const open = doorModes.includes(beh.open) ? beh.open : "approach"}
      <p>{t("behavior.door.open")}</p>
      <select
        class="{field} w-full"
        value={open}
        onchange={(e) => setBehavior({ open: e.currentTarget.value })}
      >
        {#each doorModes as m}<option value={m}
            >{t(`behavior.door.${m}`)}</option
          >{/each}
      </select>
      <Tip text={t(`behavior.door.${open}.tip`)} />
      {#if open === "when"}
        <Condition
          value={beh.when ?? {}}
          withAlways={false}
          tasks={b.tasks}
          actors={b.actors}
          onchange={(w) => setBehavior({ when: w })}
        />
      {/if}
      {#if open === "approach" || open === "when"}
        <div class="flex items-center gap-2">
          <span class={label}>{t("behavior.door.distance")}</span>
          <DragNumber
            value={beh.distance ?? 0}
            step={0.1}
            min={0}
            max={200}
            format={(d) => (d > 0 ? `${d.toFixed(1)} m` : "-")}
            oninput={(d) => {
              beh.distance = d;
              live("set_actor_behavior", {
                id,
                behavior: { distance: Math.max(0, d) },
              });
            }}
            onchange={(d) => {
              beh.distance = d;
              commit("set_actor_behavior", {
                id,
                behavior: { distance: Math.max(0, d) },
              });
            }}
          />
        </div>
        <Tip text={t("behavior.door.distance.tip")} />
      {/if}
      {#if open !== "locked"}
        <!-- the level behind it (the rooms of a house, exported as a level of their own): loaded when
             Jak comes, the door opening once it is -->
        <div class="flex items-center gap-2">
          <span class={label}>{t("behavior.door.loads")}</span>
          <input
            class="{field} flex-1"
            value={beh.loads ?? ""}
            placeholder={t("behavior.door.loads.none")}
            oninput={(e) =>
              (e.currentTarget.value = e.currentTarget.value.replace(
                /\s/g,
                "",
              ))}
            onchange={(e) =>
              setBehavior({ loads: e.currentTarget.value || null })}
          />
        </div>
        <Tip text={t("behavior.door.loads.tip")} />
      {/if}
    {:else if b.kind === "bouncer"}
      <p>{t("behavior.bouncer.active")}</p>
      <Condition
        value={beh.active ?? {}}
        withAlways
        tasks={b.tasks}
        actors={b.actors}
        onchange={(a) => setBehavior({ active: a })}
      />
      <div class="flex items-center gap-2">
        <span class={label}>{t("behavior.bouncer.height")}</span>
        <DragNumber
          value={b.bouncer_height}
          step={0.1}
          min={0}
          max={200}
          digits={1}
          unit="m"
          oninput={(h) => {
            b.bouncer_height = h;
            live(
              "set_actor_param",
              lumpArgs("spring-height", Math.max(0, h) * kUnits),
            );
          }}
          onchange={(h) => {
            b.bouncer_height = h;
            commit(
              "set_actor_param",
              lumpArgs("spring-height", Math.max(0, h) * kUnits),
            );
          }}
        />
      </div>
      <Tip text={t("behavior.bouncer.height.tip")} />
    {:else if b.kind === "elevator"}
      <p>{t("behavior.elevator.leave")}</p>
      <Condition
        value={beh.leave ?? {}}
        withAlways
        tasks={b.tasks}
        actors={b.actors}
        onchange={(l) => setBehavior({ leave: l })}
      />
      <div class="flex items-center gap-2">
        <span class={label}>{t("behavior.elevator.speed")}</span>
        <DragNumber
          value={b.elevator_speed}
          step={0.05}
          min={0.1}
          max={100}
          unit="m/s"
          oninput={(v) => {
            b.elevator_speed = v;
            live(
              "set_actor_param",
              lumpArgs("elevator-move-rate", Math.max(0.1, v) * kUnits),
            );
          }}
          onchange={(v) => {
            b.elevator_speed = v;
            commit(
              "set_actor_param",
              lumpArgs("elevator-move-rate", Math.max(0.1, v) * kUnits),
            );
          }}
        />
      </div>
    {/if}

    {#if b.crate_type !== null}
      <!-- what kind of crate, what it holds -->
      <div class="flex items-center gap-2">
        <span class={label}>{t("behavior.crate.type")}</span>
        <select
          class="{field} flex-1"
          value={b.crate_type}
          onchange={(e) =>
            commit("set_actor_param", {
              id,
              tag: "crate-type",
              value: `'${e.currentTarget.value}`,
              type: "symbol",
              label: "undo.behavior",
            })}
        >
          {#if !crateLooks.includes(b.crate_type)}<option
              value={b.crate_type}
              hidden>{b.crate_type}</option
            >{/if}
          {#each crateLooks as k}<option value={k}>{k}</option>{/each}
        </select>
      </div>
    {/if}

    <!-- there or not -->
    <p>{t("behavior.present")}</p>
    <div>
      <Condition
        value={beh.present ?? {}}
        withAlways
        tasks={b.tasks}
        actors={b.actors}
        onchange={(p) => setBehavior({ present: p })}
      />
    </div>
    <Tip text={t("behavior.present.tip")} />
  </div>

  {#if b.params.length}
    <details class="mt-3">
      <summary
        class="cursor-pointer border-b border-white/10 pb-1 text-xs font-semibold tracking-wide text-neutral-400 uppercase"
      >
        {t("params.title", b.params.length)}
      </summary>
      <p class="{muted} my-1.5">{t("params.help")}</p>
      <div class="flex flex-col gap-1">
        {#each b.params as p (p.tag)}
          {@const v = numbersOf(p.value)}
          <div class="flex items-center gap-1.5">
            <span class={label} title={p.tag}>{p.tag}</span>
            <div class="flex min-w-0 flex-1 items-center gap-1">
              {#if p.kind === "float"}
                {@const k = p.meters ? kUnits : 1}
                <DragNumber
                  value={(v[0] ?? 0) / k}
                  step={p.meters ? 0.1 : 0.01}
                  digits={p.meters ? 2 : 3}
                  unit={p.meters ? "m" : ""}
                  oninput={(f) => {
                    p.value = f * k;
                    live("set_actor_param", { id, tag: p.tag, value: f * k });
                  }}
                  onchange={(f) => {
                    p.value = f * k;
                    commit("set_actor_param", { id, tag: p.tag, value: f * k });
                  }}
                />
              {:else if p.kind === "floats" || p.kind === "ints"}
                <input
                  class="{field} w-full"
                  value={v.map(g).join(" ")}
                  onchange={(e) => list(p, e.currentTarget.value)}
                />
                <Tip text={t("params.list.tip")} />
              {:else if p.kind === "int" || p.kind.startsWith("enum:")}
                {@const i = Math.round(v[0] ?? 0)}
                {#if p.enum?.bitfield}
                  <!-- the bits by their names -->
                  <div class="flex flex-wrap gap-x-2">
                    {#each p.enum.values as [name, k]}
                      <label class="flex items-center gap-1 text-xs">
                        <input
                          type="checkbox"
                          class={check}
                          checked={bit(i, k)}
                          onchange={(e) =>
                            setParam(
                              p.tag,
                              i + (e.currentTarget.checked ? 1 : -1) * 2 ** k,
                            )}
                        />
                        {name}
                      </label>
                    {/each}
                  </div>
                {:else if p.enum}
                  <select
                    class="{field} w-full"
                    value={i}
                    onchange={(e) => setParam(p.tag, +e.currentTarget.value)}
                  >
                    {#if !p.enum.values.some(([, n]) => n === i)}<option
                        value={i}
                        hidden>?</option
                      >{/if}
                    {#each p.enum.values as [name, n]}<option value={n}
                        >{name}</option
                      >{/each}
                  </select>
                {:else}
                  <input
                    type="number"
                    step="1"
                    class="{field} w-full text-right"
                    value={i}
                    onchange={(e) =>
                      setParam(p.tag, Math.round(+e.currentTarget.value))}
                  />
                {/if}
              {:else if p.kind === "symbol" || p.kind === "string"}
                {@const s = typeof p.value === "string" ? p.value : ""}
                <input
                  class="{field} w-full"
                  value={p.kind === "symbol" ? s.replace(/^'/, "") : s}
                  onchange={(e) =>
                    setParam(
                      p.tag,
                      p.kind === "symbol"
                        ? `'${e.currentTarget.value}`
                        : e.currentTarget.value,
                    )}
                />
              {:else if p.kind === "actor"}
                {@const current = actorName(p)}
                <select
                  class="{field} w-full"
                  value={current}
                  onchange={(e) => setParam(p.tag, e.currentTarget.value)}
                >
                  <option value={current} hidden>{current || "-"}</option>
                  {#each b.actors.filter((a) => a.id !== id) as a}<option
                      value={a.name}>{a.name}</option
                    >{/each}
                </select>
              {:else if p.kind === "eco-info"}
                <!-- what it holds: a pickup and how many -->
                {@const pick = v[0] ?? 0}
                {@const amount = v.length > 1 ? v[1] : 1}
                <select
                  class="{field} w-[65%]"
                  value={pick}
                  onchange={(e) =>
                    setParam(p.tag, [+e.currentTarget.value, amount])}
                >
                  {#if !p.enum?.values.some(([, n]) => n === pick)}<option
                      value={pick}
                      hidden>?</option
                    >{/if}
                  {#each p.enum?.values ?? [] as [name, n]}<option value={n}
                      >{name}</option
                    >{/each}
                </select>
                <input
                  type="number"
                  class="{field} w-[35%] text-right"
                  value={amount}
                  onchange={(e) =>
                    setParam(p.tag, [
                      pick,
                      Math.max(0, Math.round(+e.currentTarget.value)),
                    ])}
                />
              {:else if p.kind === "vector"}
                {@const f = [0, 1, 2, 3].map((k) => v[k] ?? 0)}
                <!-- two by two: four numbers of 0.000 do not fit on a line -->
                <div class="grid w-full grid-cols-2 gap-1">
                  {#each f as x, k}
                    <DragNumber
                      value={x}
                      step={1}
                      digits={3}
                      oninput={(n) => {
                        f[k] = n;
                        p.value = [...f];
                        live("set_actor_param", {
                          id,
                          tag: p.tag,
                          value: [...f],
                        });
                      }}
                      onchange={(n) => {
                        f[k] = n;
                        p.value = [...f];
                        commit("set_actor_param", {
                          id,
                          tag: p.tag,
                          value: [...f],
                        });
                      }}
                    />
                  {/each}
                </div>
              {:else}
                <!-- paths, scripts, groups of actors: shown, not edited here -->
                <span class={muted}>
                  {t(
                    p.kind === "path"
                      ? "params.path"
                      : p.kind === "script"
                        ? "params.script"
                        : "params.other",
                    Array.isArray(p.value) ? p.value.length : 0,
                  )}
                </span>
                {#if p.kind === "script" && typeof p.value === "string"}<Tip
                    text={p.value}
                  />{/if}
              {/if}
            </div>
            <!-- back to the game's value -->
            {#if p.set && !["path", "script", "data", "actor-groups"].includes(p.kind)}
              <button class={small} onclick={() => setParam(p.tag, null)}
                >x</button
              >
              <Tip text={t("params.reset.tip")} />
            {/if}
          </div>
        {/each}
      </div>
    </details>
  {/if}
{/if}

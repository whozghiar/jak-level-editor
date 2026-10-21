<script lang="ts">
  // A condition of a behavior (ui_condition, behavior_panel.cpp): always, never, after a task of the
  // story, once an actor was activated or is gone. Its kind is shown as stored (the C++ read it back
  // through condition_of, which turns a kind with nothing chosen yet into "always": its choice of a
  // task or an actor never stayed open).
  import { t } from "../../lib/i18n";
  import Tip from "../common/Tip.svelte";
  import { escapeReverts, field } from "./edit.svelte";

  type Value = { when?: string; arg?: string };
  let {
    value,
    withAlways,
    tasks,
    actors,
    onchange,
  }: {
    value: Value;
    withAlways: boolean;
    tasks: string[];
    actors: { name: string }[];
    onchange: (v: Value) => void;
  } = $props();

  const kinds = ["always", "never", "task", "pressed", "gone"];
  const kind = $derived(
    kinds.includes(value?.when ?? "") ? value.when! : "always",
  );
  const arg = $derived(typeof value?.arg === "string" ? value.arg : "");
  const json = (when: string, arg: string): Value =>
    arg ? { when, arg } : { when };
  const uid = $props.id();
  const list = `tasks-${uid}`;
</script>

<div class="flex flex-col gap-1">
  <select
    class="{field} w-full"
    value={kind}
    onchange={(e) =>
      e.currentTarget.value !== kind &&
      onchange(json(e.currentTarget.value, ""))}
  >
    {#each kinds as k}
      <option value={k} hidden={k === "always" && !withAlways}
        >{t(`behavior.when.${k}`)}</option
      >
    {/each}
  </select>
  {#if kind === "task"}
    <!-- a task node of the story: closed once the player did that step -->
    <input
      use:escapeReverts
      class="{field} w-full"
      {list}
      value={arg}
      placeholder={t("behavior.choose_task")}
      onchange={(e) =>
        tasks.includes(e.currentTarget.value) &&
        onchange(json("task", e.currentTarget.value))}
    />
    <datalist id={list}>
      {#each tasks as name}<option value={name}></option>{/each}
    </datalist>
    <Tip text={t("behavior.task.tip")} />
  {:else if kind === "pressed" || kind === "gone"}
    <!-- another actor of the document, by its name -->
    <select
      class="{field} w-full"
      value={arg}
      onchange={(e) => onchange(json(kind, e.currentTarget.value))}
    >
      {#if !arg}<option value="" hidden>{t("behavior.choose_actor")}</option
        >{:else if !actors.some((a) => a.name === arg)}<option value={arg} hidden
          >{arg}</option
        >{/if}
      {#each actors as a}<option value={a.name}>{a.name}</option>{/each}
    </select>
    <Tip
      text={t(
        kind === "pressed" ? "behavior.pressed.tip" : "behavior.gone.tip",
      )}
    />
  {/if}
</div>

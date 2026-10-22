<script lang="ts">
  // The menu bar (ui_menu in panels.cpp): every item of the C++ editor, its keys, when it can be used.
  import IconCheck from "~icons/mdi/check";
  import IconChevronRight from "~icons/mdi/chevron-right";
  import {
    changeLanguage,
    frameView,
    importModel,
    newLevel,
    openProject,
    openProjectFile,
    playLevel,
    quit,
    save,
    toggleView,
    ui,
  } from "../../lib/actions.svelte";
  import { engine } from "../../lib/engine.svelte";
  import { language, languages, t } from "../../lib/i18n";
  import { call } from "../../lib/rpc";
  import { windows } from "../../lib/windows.svelte";
  import Tip from "../common/Tip.svelte";

  type Item = {
    label: string;
    keys?: string;
    enabled?: boolean;
    checked?: boolean;
    tip?: string;
    guide?: string;
    run?: () => void;
    items?: (Item | null)[];
  };
  type Menu = { label: string; enabled?: boolean; items: (Item | null)[] }; // null: a separator

  // the shapes of the Build menu (shape_presets in scene/shapes.cpp)
  const shapes = [
    "box",
    "floor",
    "wall",
    "doorway",
    "window",
    "pillar",
    "ramp",
    "stairs",
    "roof",
    "roof-hip",
    "cylinder",
    "ground",
  ];

  const s = $derived(engine.state);
  const sel = $derived(s.selection.count > 0);
  const doc = $derived(s.document.open);
  const group = $derived(s.selection.group);
  const v = $derived(s.view);
  const view = (key: string, label: string, tip?: string): Item => ({
    label: t(label),
    checked: !!v[key],
    tip: tip && t(tip),
    run: () => toggleView(key),
  });

  const menus: Menu[] = $derived([
    {
      label: t("menu.file"),
      items: [
        { label: t("menu.file.extract"), run: () => windows.open("extract") },
        null,
        { label: t("menu.file.new"), keys: "Ctrl+N", run: newLevel },
        { label: t("menu.file.open"), keys: "Ctrl+O", run: openProject },
        {
          label: t("menu.file.recent"),
          enabled: s.recent_projects.length > 0,
          items: s.recent_projects.map((p) => ({
            label: p,
            run: () => openProjectFile(p),
          })),
        },
        {
          label: t("menu.file.save"),
          keys: "Ctrl+S",
          enabled: doc,
          run: () => save(false),
        },
        {
          label: t("menu.file.save_as"),
          keys: "Ctrl+Shift+S",
          enabled: doc,
          run: () => save(true),
        },
        null,
        { label: t("menu.file.import_model"), enabled: doc, run: importModel },
        null,
        { label: t("menu.file.settings"), run: () => windows.open("settings") },
        null,
        { label: t("menu.file.quit"), run: quit },
      ],
    },
    {
      label: t("menu.edit"),
      items: [
        {
          label: t("menu.edit.undo"),
          keys: "Ctrl+Z",
          enabled: s.history.can_undo,
          tip: s.history.undo_label,
          run: () => call("undo"),
        },
        {
          label: t("menu.edit.redo"),
          keys: "Ctrl+Y",
          enabled: s.history.can_redo,
          tip: s.history.redo_label,
          run: () => call("redo"),
        },
        null,
        {
          label: t("menu.edit.copy"),
          keys: "Ctrl+C",
          enabled: sel,
          run: () => call("copy_selection"),
        },
        {
          label: t("menu.edit.paste"),
          keys: "Ctrl+V",
          enabled: !s.clipboard_empty,
          run: () => call("paste"),
        },
        {
          label: t("menu.edit.duplicate"),
          keys: "Ctrl+D",
          enabled: sel,
          run: () => call("duplicate_selection"),
        },
        {
          label: t("menu.edit.delete"),
          keys: t("key.delete"),
          enabled: sel,
          run: () => call("delete_selection"),
        },
        null,
        {
          label: t("menu.edit.create_prefab"),
          enabled: sel,
          run: () => windows.open("new_prefab"),
        },
        {
          label: t("menu.edit.select_none"),
          keys: t("key.escape"),
          enabled: sel,
          run: () => call("select_only", { id: 0 }),
        },
        {
          label: t("menu.edit.select_level"),
          keys: "Ctrl+L",
          enabled: sel,
          run: () => call("select_level", { group: s.selection.level }),
        },
        {
          label: t("menu.edit.select_assembly"),
          keys: "Ctrl+G",
          enabled: sel,
          tip: t("menu.edit.select_assembly.tip"),
          run: () => call("select_assembly", { id: s.selection.primary }),
        },
        {
          label: t("menu.edit.select_group"),
          keys: t("key.double_click"),
          enabled: sel && !!group,
          tip: t("menu.edit.select_group.tip"),
          run: () => call("select_group", { id: group }),
        },
        null,
        {
          label: t("menu.edit.group"),
          keys: "Ctrl+Shift+G",
          enabled: sel,
          tip: t("group.make.tip"),
          run: () => windows.open("new_group"),
        },
        {
          label: t("group.ungroup"),
          enabled: sel && !!group,
          run: () => call("ungroup", { id: group }),
        },
      ],
    },
    {
      label: t("menu.view"),
      items: [
        {
          label: t("menu.view.frame_selection"),
          keys: "F",
          enabled: sel,
          run: () => frameView("selection"),
        },
        {
          label: t("menu.view.frame_all"),
          keys: t("key.home"),
          run: () => frameView("all"),
        },
        null,
        // the level drawn by the game's own renderer, lit at an hour of the day
        {
          label: t("menu.view.game_view"),
          keys: "G",
          enabled: v.game_view_ready,
          checked: v.game_view,
          tip: v.game_view_ready
            ? t("menu.view.game_view.tip")
            : t("menu.view.game_view.unavailable", v.game_view_error),
          run: () => toggleView("game_view"),
        },
        // its sky, hour and weather: in a window of their own
        {
          label: t("menu.view.game_window"),
          enabled: v.game_view_ready,
          checked: windows.isOpen("game"),
          tip: t("menu.view.game_window.tip"),
          run: () => windows.toggle("game"),
        },
        null,
        view("effects", "menu.view.effects", "menu.view.effects.tip"),
        view("actors", "menu.view.actors"),
        view("ocean", "menu.view.ocean"),
        view("fog", "menu.view.fog"),
        view("navmesh", "menu.view.navmesh"),
        view("labels", "menu.view.labels"),
        view("grid", "menu.view.grid"),
        view("textures", "menu.view.textures"),
        view(
          "select_terrain",
          "menu.view.select_terrain",
          "menu.view.select_terrain.tip",
        ),
        {
          label: t("menu.view.log"),
          checked: windows.isOpen("log"),
          run: () => windows.toggle("log"),
        },
        null,
        {
          label: t("menu.view.language"),
          items: languages.map((l) => ({
            label: l.name,
            checked: l.code === language(),
            run: () => changeLanguage(l.code),
          })),
        },
      ],
    },
    // Build: the shapes, the palette of materials, a model of one's own
    {
      label: t("menu.create"),
      enabled: doc,
      items: [
        ...shapes.map((p) => ({
          label: t(`menu.create.${p}`),
          run: () => call("start_shape_placement", { preset: p, once: false }),
        })),
        null,
        {
          label: t("menu.create.materials"),
          run: () => (ui.leftTab = "build"),
        },
        { label: t("menu.file.import_model"), run: importModel },
      ],
    },
    // Level: the levels of the document, the one it replaces in the game, nav meshes, the export
    {
      label: t("menu.level"),
      enabled: doc,
      items: [
        { label: t("menu.level.add"), run: () => (ui.leftTab = "levels") },
        {
          label: t("menu.level.select"),
          keys: "Ctrl+L",
          run: () => call("select_level", { group: 0 }),
        },
        {
          label: t("menu.level.replace"),
          guide: "replace",
          run: () => {
            call("select_only", { id: 0 });
            ui.scrollToReplace = true;
          },
        },
        {
          label: t("menu.level.restore_effects"),
          tip: t("menu.level.restore_effects.tip"),
          run: () => call("restore_level_effects"),
        },
        {
          label: t("menu.level.navmesh"),
          checked: windows.isOpen("navmesh"),
          run: () => windows.toggle("navmesh"),
        },
        {
          label: t("menu.level.gaps"),
          checked: windows.isOpen("gaps"),
          tip: t("menu.level.gaps.tip"),
          run: () => windows.toggle("gaps"),
        },
        null,
        {
          label: t("menu.file.export_game"),
          guide: "export",
          run: () => windows.open("export"),
        },
        // only with a game runtime the host gives (no runtime: the export and its README)
        ...(s.runtime?.available || s.runtime?.tooling
          ? [
              {
                label: t("menu.level.play"),
                tip: t("menu.level.play.tip"),
                run: playLevel,
              },
            ]
          : []),
      ],
    },
    {
      label: t("menu.help"),
      items: ["menu.help.tour", "menu.help.tools", "menu.help.keys"].map(
        (key, tab) => ({
          label: t(key),
          keys: tab === 0 ? "F1" : undefined,
          run: () => windows.open("guide", { tab }),
        }),
      ),
    },
  ]);

  let opened = $state(-1);
  let bar: HTMLElement;

  function run(it: Item) {
    opened = -1;
    it.run?.();
  }

  const row =
    "flex w-full items-center gap-2 px-3 py-1 text-left text-sm whitespace-nowrap enabled:hover:bg-orange-500/15 enabled:hover:text-white disabled:text-neutral-600";
</script>

<svelte:window
  onpointerdown={(e) =>
    opened >= 0 && !bar.contains(e.target as Node) && (opened = -1)}
  onkeydown={(e) =>
    opened >= 0 && e.key === "Escape" && (e.preventDefault(), (opened = -1))}
/>

{#snippet list(items: (Item | null)[])}
  <div
    class="min-w-60 rounded-md border border-white/10 bg-[#101010] py-1 text-neutral-200 shadow-xl shadow-black/60"
  >
    {#each items as it}
      {#if !it}
        <div class="my-1 border-t border-white/5"></div>
      {:else if it.items}
        <div class="group/sub relative">
          <button class={row} disabled={it.enabled === false}>
            <span class="w-4"></span>
            <span class="flex-1">{it.label}</span>
            <IconChevronRight class="text-neutral-500" />
          </button>
          {#if it.enabled !== false}
            <div
              class="absolute top-0 left-full hidden max-h-[70vh] overflow-y-auto group-hover/sub:block"
            >
              {@render list(it.items)}
            </div>
          {/if}
        </div>
      {:else}
        <span class="block">
          <button
            class={row}
            disabled={it.enabled === false}
            onclick={() => run(it)}
          >
            <span class="w-4 text-orange-400"
              >{#if it.checked}<IconCheck />{/if}</span
            >
            <span class="flex-1">{it.label}</span>
            {#if it.keys}<span class="pl-6 text-xs text-neutral-500"
                >{it.keys}</span
              >{/if}
          </button>
        </span>
        <Tip guide={it.guide} text={it.tip} placement="right" />
      {/if}
    {/each}
  </div>
{/snippet}

<nav
  bind:this={bar}
  class="relative z-40 flex h-8 shrink-0 items-center border-b border-white/5 bg-[#0b0b0b] px-1 text-sm"
>
  {#each menus as m, i}
    <div class="relative">
      <button
        class="rounded px-3 py-1 enabled:hover:bg-white/10 disabled:text-neutral-600 {opened ===
        i
          ? 'bg-white/10 text-orange-400'
          : 'text-neutral-200'}"
        disabled={m.enabled === false}
        onclick={() => (opened = opened === i ? -1 : i)}
        onpointerenter={() => opened >= 0 && (opened = i)}
      >
        {m.label}
      </button>
      {#if opened === i}
        <div class="absolute top-full left-0 mt-1">{@render list(m.items)}</div>
      {/if}
    </div>
  {/each}
</nav>

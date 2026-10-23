<script lang="ts">
  // The whole editor (the C++ App's interface, draw_ui in panels.cpp): the app mounts it, as a host
  // application could. Title bar, menus, toolbar, the left panel's tabs, the viewport, the inspector,
  // the status bar, the floating windows, the toasts, the keys.
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Toast } from "flowbite-svelte";
  import { onMount, untrack, type Component } from "svelte";
  import { fly } from "svelte/transition";
  import IconAlert from "~icons/mdi/alert-circle";
  import IconCheck from "~icons/mdi/check";
  import IconGroup from "~icons/mdi/group";
  import IconHammer from "~icons/mdi/hammer";
  import IconMap from "~icons/mdi/map";
  import IconPackage from "~icons/mdi/package-variant";
  import IconError from "~icons/mdi/stop-alert";
  import IconGrid from "~icons/mdi/view-grid";
  import IconMinimize from "~icons/mdi/window-minimize";
  import IconMaximize from "~icons/mdi/window-maximize";
  import IconClose from "~icons/mdi/window-close";
  import logo from "../src-tauri/icons/32x32.png";
  import Tip from "./components/common/Tip.svelte";
  import Window, { dragPointer } from "./components/common/Window.svelte";
  import MenuBar from "./components/frame/MenuBar.svelte";
  import StoryBar from "./components/frame/StoryBar.svelte";
  import Toolbar from "./components/frame/Toolbar.svelte";
  import Inspector from "./components/inspector/Inspector.svelte";
  import BuildPanel from "./components/panels/BuildPanel.svelte";
  import CatalogPanel from "./components/panels/CatalogPanel.svelte";
  import GroupsPanel from "./components/panels/GroupsPanel.svelte";
  import LevelsPanel from "./components/panels/LevelsPanel.svelte";
  import PrefabsPanel from "./components/panels/PrefabsPanel.svelte";
  import {
    confirmDiscard,
    openProjectFile,
    quit,
    ui,
    type LeftTab,
  } from "./lib/actions.svelte";
  import { engine, startEngine } from "./lib/engine.svelte";
  import { initLanguage, setLanguage, t } from "./lib/i18n";
  import { call, onEngine, toasts } from "./lib/rpc";
  import { handleShortcut } from "./lib/shortcuts";
  import { registry, windows } from "./lib/windows.svelte";
  import Viewport from "$level-editor/render/Viewport.svelte";

  // embedded: a host that frames the window and gives the editor a box of a fixed height (no title
  // bar of its own; a host whose window has no frame mounts it without); locale: the host's
  // ("fr-FR"), for the session only (the standalone editor's language stays the one chosen)
  let { embedded = false, locale }: { embedded?: boolean; locale?: string } =
    $props();

  // a host's locale goes to the engine; else the engine's language wins (the session's --lang,
  // else the one chosen before, else the system's: apply_options), the one shown first only a guess
  $effect.pre(() => {
    const code = initLanguage(locale);
    untrack(() =>
      locale
        ? call("set_language", { code, keep: false }, true)
        : call<{ language?: string }>("get_state", {}, true).then(
            (s) => s?.language && setLanguage(s.language),
          ),
    );
  });

  const tauri = "__TAURI_INTERNALS__" in window;
  const s = $derived(engine.state);
  const title = $derived(s.document.title || t("title.untitled"));
  const unsaved = $derived(s.document.open && s.document.unsaved);

  onMount(() => {
    const stop = startEngine();
    if (!tauri) return stop;
    let alive = true;
    const offs: (() => void)[] = [];
    // the remote control's search for holes shows in the Holes window (show_gaps_window = true)
    offs.push(
      onEngine(
        "jle:changed",
        (p) => p?.show_gaps && !windows.isOpen("gaps") && windows.open("gaps"),
      ),
    );
    // an extraction finished: the Levels panel shows what was written (left_tab_request = 0)
    offs.push(
      onEngine("jle:changed", (p) => p?.extracted && (ui.leftTab = "levels")),
    );
    const keep = (p: Promise<() => void>) =>
      p.then((off) => (alive ? offs.push(off) : off()));
    const win = getCurrentWindow();
    // closing the window (its cross, File > Quit, Alt+F4) asks first when there are unsaved changes
    keep(
      win.onCloseRequested((e) => {
        e.preventDefault();
        confirmDiscard(async () => {
          // "Don't save": the engine may outlive the window (a host application): the changes go
          if (engine.state.document.unsaved) await call("close_document");
          await Promise.all(offs.map((off) => off()));
          offs.length = 0;
          await win.close();
        });
      }),
    );
    // a project dropped on the window opens; a folder is taken as the library (process_event)
    keep(
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type !== "drop" || !e.payload.paths.length) return;
        const path = e.payload.paths[0];
        void call<string>("drop_path", { path }).then(
          (r) => r === "file" && openProjectFile(path),
        );
      }),
    );
    return () => {
      alive = false;
      stop();
      offs.forEach((off) => off());
    };
  });

  // the window's title (set_title in app.cpp)
  $effect(() => {
    const text = `${title}${unsaved ? " *" : ""} - ${t("app.title")}`;
    if (tauri && !embedded) getCurrentWindow().setTitle(text);
  });

  // the terrain brush's window: open while the tool is
  $effect(() => {
    const on = s.tool === "terrain";
    untrack(() =>
      on
        ? !windows.isOpen("terrain") && windows.open("terrain")
        : windows.close("terrain"),
    );
  });

  // the left panel's tabs, in the C++ editor's order
  const tabs: {
    id: LeftTab;
    key: string;
    guide: string;
    icon: Component;
    panel: Component;
  }[] = [
    {
      id: "levels",
      key: "tab.levels",
      guide: "levels",
      icon: IconMap,
      panel: LevelsPanel,
    },
    {
      id: "build",
      key: "tab.build",
      guide: "build",
      icon: IconHammer,
      panel: BuildPanel,
    },
    {
      id: "catalog",
      key: "tab.catalog",
      guide: "catalog",
      icon: IconGrid,
      panel: CatalogPanel,
    },
    {
      id: "prefabs",
      key: "tab.prefabs",
      guide: "game_prefabs",
      icon: IconPackage,
      panel: PrefabsPanel,
    },
    {
      id: "groups",
      key: "tab.groups",
      guide: "groups",
      icon: IconGroup,
      panel: GroupsPanel,
    },
  ];
  const Panel = $derived(tabs.find((x) => x.id === ui.leftTab)!.panel);

  // the panels' widths, as the C++ clamps them
  let width = $state(1600);
  let left = $state(340);
  let right = $state(320);
  const leftW = $derived(Math.max(260, Math.min(left, width * 0.45)));
  const rightW = $derived(Math.max(260, Math.min(right, width * 0.4)));

  const statusKey = $derived(
    `status.${s.tool === "place" || s.tool === "topology" || s.tool === "terrain" ? s.tool : "select"}`,
  );
  const last = $derived(engine.log.at(-1));
  const logColor = {
    info: "text-neutral-400",
    warn: "text-amber-300",
    error: "text-red-400",
  };
  const jobs = $derived(Object.values(engine.jobs));

  const winButton =
    "flex h-full w-12 items-center justify-center hover:bg-white/10";
</script>

<div
  class="relative flex {embedded
    ? 'h-full'
    : 'h-screen'} w-full flex-col overflow-hidden bg-[#141414] text-white select-none"
  bind:clientWidth={width}
>
  {#if !embedded}
    <header
      class="flex h-8 shrink-0 items-center bg-[#101010] pl-2"
      data-tauri-drag-region
    >
      <div class="pointer-events-none flex shrink-0 items-center gap-2">
        <img class="h-5" src={logo} alt="" />
        <p class="text-base font-black tracking-tight">{t("app.title")}</p>
      </div>
      <div
        class="pointer-events-none mx-3 h-6 shrink-0 border-l border-neutral-800"
      ></div>
      <p
        class="pointer-events-none flex min-w-0 items-center gap-2 font-mono text-sm text-neutral-400"
      >
        <span class="truncate">{title}</span>
        {#if unsaved}<span class="size-2 shrink-0 rounded-full bg-orange-500"
          ></span>{/if}
      </p>
      <div class="ml-auto flex shrink-0 self-stretch">
        <button
          class={winButton}
          aria-label={t("window.minimize")}
          onclick={() => tauri && getCurrentWindow().minimize()}
        >
          <IconMinimize />
        </button>
        <button
          class={winButton}
          aria-label={t("window.maximize")}
          onclick={() => tauri && getCurrentWindow().toggleMaximize()}
        >
          <IconMaximize />
        </button>
        <button
          class="{winButton} hover:bg-red-600"
          aria-label={t("window.close")}
          onclick={quit}
        >
          <IconClose />
        </button>
      </div>
    </header>
  {/if}

  <MenuBar />
  <Toolbar />

  <div class="flex min-h-0 flex-1">
    <!-- the left panel: its tabs as the launcher's sidebar -->
    <nav
      class="flex w-14 shrink-0 flex-col items-center gap-3 border-r border-white/5 bg-[#0b0b0b] py-3"
    >
      {#each tabs as tab}
        <div class="relative flex w-full justify-center">
          {#if ui.leftTab === tab.id}
            <div
              class="absolute top-1/2 left-0 h-8 w-0.75 -translate-y-1/2 rounded-r bg-orange-500"
            ></div>
          {/if}
          <button
            class="flex h-10 w-10 items-center justify-center rounded-lg text-2xl transition-colors {ui.leftTab ===
            tab.id
              ? 'text-white'
              : 'text-neutral-500 hover:text-neutral-200'}"
            aria-label={t(tab.key)}
            onclick={() => (ui.leftTab = tab.id)}
          >
            <tab.icon />
          </button>
          <Tip guide={tab.guide} placement="right" />
        </div>
      {/each}
    </nav>
    <aside
      class="min-w-0 shrink-0 overflow-y-auto bg-[#141414]"
      style:width="{leftW}px"
    >
      <Panel />
    </aside>
    <div
      class="w-1 shrink-0 cursor-col-resize bg-white/5 hover:bg-orange-500/60"
      role="separator"
      onpointerdown={(e) => dragPointer(e, (dx) => (left = leftW + dx))}
    ></div>

    <main class="relative min-w-0 flex-1 bg-[#0b0b0b]">
      <Viewport />
      {#if s.story.available}
        <div class="absolute inset-x-2 bottom-2"><StoryBar /></div>
      {/if}
    </main>

    <div
      class="w-1 shrink-0 cursor-col-resize bg-white/5 hover:bg-orange-500/60"
      role="separator"
      onpointerdown={(e) => dragPointer(e, (dx) => (right = rightW - dx))}
    ></div>
    <aside
      class="min-w-0 shrink-0 overflow-y-auto bg-[#141414]"
      style:width="{rightW}px"
    >
      <Inspector />
    </aside>
  </div>

  <!-- the status bar (ui_statusbar) -->
  <footer
    class="flex h-7 shrink-0 items-center gap-2 border-t border-white/5 bg-[#101010] px-3 text-xs whitespace-nowrap text-neutral-400"
  >
    <span class="min-w-0 shrink truncate">{t(statusKey)}</span>
    {#if unsaved}<span class="shrink-0 text-amber-300"
        >| {t("status.unsaved")}</span
      >{/if}
    {#if s.loading}<span class="shrink-0 text-sky-300"
        >| {t("status.loading")}</span
      >{/if}
    {#if s.remote_clients > 0}
      <span class="shrink-0 text-violet-300">| {t("status.remote")}</span>
      <Tip text={t("status.remote.tip")} placement="top" />
    {/if}
    {#if last}
      <button
        class="min-w-0 shrink truncate text-left {logColor[last.level] ??
          logColor.info} hover:underline"
        onclick={() => windows.open("log")}
      >
        | {last.text}
      </button>
    {/if}
    <div class="ml-auto flex shrink-0 items-center gap-4">
      {#each jobs as j (j.id)}
        <span class="flex items-center gap-2">
          {t(j.label)}
          <span class="h-1.5 w-24 overflow-hidden rounded bg-white/10">
            <span
              class="block h-full bg-orange-500"
              style:width="{j.total ? (100 * j.done) / j.total : 0}%"
            ></span>
          </span>
        </span>
      {/each}
    </div>
  </footer>

  <!-- the floating windows, kept inside the editor under its toolbar (the title bar h-8, the menus
       h-8, the toolbar h-11) -->
  <div
    class="pointer-events-none absolute inset-x-0 bottom-0 z-30 {embedded
      ? 'top-[76px]'
      : 'top-[108px]'}"
  >
    {#each windows.list as w (w.id)}
      {@const def = registry[w.id]}
      <Window
        title={t(def.title)}
        width={def.width}
        height={def.height}
        modal={def.modal}
        anchor={def.anchor}
        z={w.z}
        onclose={() => windows.close(w.id, true)}
        onfocus={() => windows.focus(w.id)}
      >
        {#key w.opened}
          <def.component {...w.props} />
        {/key}
      </Window>
    {/each}
  </div>

  <!-- the toasts, as the launcher's -->
  <div
    class="pointer-events-none absolute right-4 bottom-10 z-50 flex flex-col items-end gap-2"
  >
    {#each [...toasts] as [id, m] (id)}
      <Toast
        dismissable={false}
        class="pointer-events-auto bg-gray-800 text-wrap text-gray-300 shadow-lg shadow-black/50 dark:bg-gray-800 dark:text-gray-300"
        classes={{ content: "w-full text-sm font-normal overflow-hidden" }}
        transition={fly}
        params={{ x: 200 }}
      >
        {#snippet icon()}
          {#if m.level === "error"}
            <IconError class="text-3xl text-red-500" />
          {:else if m.level === "warn"}
            <IconAlert class="text-3xl text-orange-500" />
          {:else}
            <IconCheck class="text-3xl text-green-500" />
          {/if}
        {/snippet}
        <p class="text-sm font-semibold">{m.msg}</p>
      </Toast>
    {/each}
  </div>
</div>

<svelte:window onkeydown={handleShortcut} />

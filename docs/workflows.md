# GitHub Actions workflows

What the three workflows of [`.github/workflows/`](../.github/workflows/) do, when they run, and how
to run the same checks on your computer before you push.

A **workflow** is a list of jobs that GitHub runs on its own computers (the *runners*: Windows and
Linux machines started for the occasion and thrown away afterwards), written in a YAML file. Their
runs are in the **Actions** tab of the repository. A green tick next to a commit means they all
passed; a red cross means one failed: click it, then the job, to read its log.

| Workflow | File | Runs | Answers |
|---|---|---|---|
| **Lint** | [`lint.yaml`](../.github/workflows/lint.yaml) | at every push on any branch, on every pull request, by hand | is the code written in the expected form? |
| **Build** | [`build.yaml`](../.github/workflows/build.yaml) | the same | does it build and pass its tests on Windows and Linux? |
| **Release** | [`release.yaml`](../.github/workflows/release.yaml) | by hand, with `patch`, `minor` or `major` | the next version `vX.Y.Z`, tagged, and its installers and zips in a draft release |

Lint and Build run side by side and independently: a formatting mistake does not hide whether the
code builds, and the other way round.

## Lint

Two jobs, on Linux:

| Job | What it runs | Fails when | Fix it with |
|---|---|---|---|
| **Rust formatting** | `cargo fmt --all --check` | a Rust file is not formatted as `rustfmt.toml` says (2 spaces) | `cargo fmt --all` |
| **Interface (formatting, types)** | `yarn install --frozen-lockfile`, `yarn check-format`, `yarn typecheck` | `yarn.lock` does not match `package.json`; a file is not formatted as Prettier wants; svelte-check finds a type error | `npx yarn install`, `npx yarn format`, the error's file and line |

Prettier checks the interface (`ui/`, `src/`), `src-tauri/`, these workflows and the configuration
files at the root ([`.prettierignore`](../.prettierignore) lists what it leaves alone: the Rust code,
the C++, the game data, the translations, the docs). svelte-check checks the TypeScript and the
Svelte components: a wrong property, a missing argument, or a call the engine does not have.

Two examples of failures, and what they mean:

- *Rust formatting*: `Diff in .../crates/core/src/editor/groups.rs:42:`, followed by the lines as
  they are and as rustfmt wants them: the code is right, only its layout differs. Run
  `cargo fmt --all` and commit the result.
- *Interface*: `ui/components/panels/GroupsPanel.svelte:40:12 Error: Argument of type '"selec"' is
  not assignable to parameter of type 'CallName'`: a call's name is misspelled; `ui/lib/api.ts`
  lists the right ones.

## Build

Two jobs, each on Windows and on Linux (`fail-fast: false`: a failure on one system does not stop
the other):

**ogle-extract** builds the extractor (C++) as the releases do: CMake, Ninja, Clang, the C runtime
inside the program on Windows (`MultiThreaded`). On Windows,
[`ilammy/msvc-dev-cmd`](https://github.com/ilammy/msvc-dev-cmd) gives Clang the headers and
libraries of Visual Studio. The job lists what it built: `build-release/bin/ogle-extract(.exe)`
and its tables `ogle-extract-data/`.

**Editor**:

1. installs Rust (stable) and Node.js 22, and on Linux the system packages of a Tauri application
   (webkit2gtk and others); the Rust build is cached between runs
   ([`Swatinem/rust-cache`](https://github.com/Swatinem/rust-cache)), so that a run does not compile
   every dependency again;
2. `cargo test -p jak-level-editor-core -p jak-level-editor-mcp`: the engine's and the MCP server's
   tests. The runners have no game: the tests that read extracted levels skip themselves;
3. `yarn install --frozen-lockfile`, then `yarn tauri build --no-bundle`: the interface (Vite) and
   the program, without making installers (the releases make them).

There is no macOS job: ogle-extract is compiled with `-mavx` (x86 only) and the editor was never
tried there.

The OpenGOAL Launcher does not build the editor: it downloads the portable zip of a published
release.

## Release

Runs by hand only (**Actions > Release > Run workflow**, on the branch to release), with what the
release changes: `patch`, `minor` or `major`. The workflow counts the version itself: **v1.0.0**
for the first release, then from the last tag `vX.Y.Z` (after v1.2.3: `patch` v1.2.4, `minor`
v1.3.0, `major` v2.0.0). It never publishes anything: it makes a **draft** release, which you
check, then publish by hand. [RELEASING.md](../RELEASING.md) gives the steps.

1. **Version and draft release** (Linux) counts the next version from the repository's tags,
   writes it into `package.json`, `Cargo.toml`, `Cargo.lock` and `src-tauri/tauri.conf.json`,
   checks the three versions, commits them to the branch (*chore: release vX.Y.Z*), tags that
   commit and pushes both at once; then creates the draft release on that tag, its notes written
   by GitHub from the commits. One release runs at a time (`concurrency: release`): two would take
   the same number.
2. **Build** (`windows-x64` and `linux-x64`):
   - ogle-extract in `build-release/`;
   - the editor and its installers with [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action)
     and `src-tauri/tauri.release.conf.json`, which puts ogle-extract in their resources; the action
     uploads the installers to the draft;
   - the MCP server, `cargo build --release -p jak-level-editor-mcp`;
   - the three zips, uploaded with `gh release upload --clobber` (a job run again replaces them).

   On Windows, `RUSTFLAGS=-C target-feature=+crt-static` puts the C runtime inside the programs:
   they run on a Windows without the Visual C++ redistributable.

The draft then holds, per platform:

| Asset | Content |
|---|---|
| `*_x64-setup.exe`, `*_x64_en-US.msi` | the Windows installers (NSIS, for the current user; MSI): the editor, `lang/`, `data/`, `ogle-extract.exe` and `ogle-extract-data/` in its folder |
| `*_amd64.deb`, `*_amd64.AppImage` | the Linux packages, the same resources |
| `jak-level-editor-portable-<platform>.zip` | at its root: `jak-level-editor(.exe)`, `lang/`, `data/`, `ogle-extract(.exe)`, `ogle-extract-data/` (the editor finds them next to itself); the OpenGOAL Launcher downloads `https://github.com/whozghiar/jak-level-editor/releases/download/v<version>/jak-level-editor-portable-<platform>.zip` |
| `ogle-extract-<platform>.zip` | at its root: `ogle-extract(.exe)` and `ogle-extract-data/` |
| `jak-level-editor-mcp-<platform>.zip` | `jak-level-editor-mcp(.exe)`, the MCP server ([mcp.md](mcp.md)): it starts the editor found next to it |

The workflow has the permission `contents: write` (to push the version's commit and its tag, create
the release and upload to it), with the run's own token: no secret to set up. Its commit and tag
start no other workflow (GitHub does not run workflows for what a run's own token pushes).

## The same checks on your computer

| Workflow | Commands, from the repository's folder |
|---|---|
| Lint | `cargo fmt --all --check`, `npx yarn check-format`, `npx yarn typecheck` |
| Build | `cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release && cmake --build build`, `cargo test -p jak-level-editor-core -p jak-level-editor-mcp`, `npx yarn tauri build --no-bundle` |
| Release | [The extraction](architecture.md#20-the-extraction) gives the build of the installers with ogle-extract |

Running Lint before you push saves a round trip: it takes seconds.

## Changing a workflow

- The files are YAML, formatted by Prettier like the interface (`npx yarn format`).
- Try a change on your branch: push it (Lint and Build run on every branch), or **Run workflow** in
  the Actions tab.
- Keep the editor's tests runnable without the game: GitHub's runners have none, and the game's
  files must never be in the repository.

# Making a release

A release is a **draft** GitHub release that `.github/workflows/release.yaml` makes, with the
editor built for Windows and Linux. You start it, check the draft, then publish it by hand.

## 1. Choose what the release changes

The versions are `vX.Y.Z`, and the workflow counts them itself from the last release's tag: the
first release is **v1.0.0**, then

| Choice | When | After v1.2.3 |
|---|---|---|
| `patch` | fixes only | v1.2.4 |
| `minor` | new features; what worked still works | v1.3.0 |
| `major` | something that worked no longer does: a project or a prefab of before no longer opens, a tool of the MCP server changes its arguments | v2.0.0 |

## 2. Run the workflow

**Actions > Release > Run workflow**, on the branch to release (`master-dev`), with that choice.
The workflow:

1. takes the last tag `vX.Y.Z` of the repository and counts the next version;
2. writes it into `package.json`, `Cargo.toml` (every crate takes it), `Cargo.lock` and
   `src-tauri/tauri.conf.json` (the installers' version), commits them to the branch (*chore:
   release vX.Y.Z*, by github-actions[bot]), tags that commit and pushes both;
3. builds the release from that tag.

Then `git pull` your branch: it lacks that commit. Never tag by hand: a tag pushed by hand starts
nothing, and the next release counts from it.

A tag whose release was published is never moved: the OpenGOAL Launcher's users download its
portable zip by that tag. A wrong release is fixed by the next
version. If the build of a release not published yet must be made again from another commit,
delete the draft and its tag (`git push origin :vX.Y.Z`): the next run takes the same number.

To try a build without releasing: the **Build** workflow builds the editor at every push
([workflows.md](docs/workflows.md)); only a release makes the installers.

## 3. Check the draft

The workflow takes 20 to 30 minutes. The draft (**Releases**, marked *Draft*) then holds, for
`windows-x64` and `linux-x64`:

| Asset | What it is |
|---|---|
| `Jak.Level.Editor_X.Y.Z_x64-setup.exe` | Windows installer (for the current user, no administrator rights) |
| `Jak.Level.Editor_X.Y.Z_x64_en-US.msi` | Windows installer (MSI, for all users) |
| `jak-level-editor_X.Y.Z_amd64.deb`, `Jak.Level.Editor_X.Y.Z_amd64.AppImage` | Linux packages |
| `jak-level-editor-portable-<platform>.zip` | the editor without installing: unzip it anywhere and run `jak-level-editor`; the OpenGOAL Launcher downloads it |
| `jak-level-editor-mcp-<platform>.zip` | the MCP server, for an AI assistant (optional): put it next to the editor's program |

Download the portable zip of your system, start the editor, and open **File > Extract game
assets...**: it must find the Jak 3 you installed with OpenGOAL (its version and its levels, in
green).

Edit the release notes GitHub wrote if you like.

## 4. Publish

**Edit** the draft, then **Publish release**. The OpenGOAL Launcher downloads
`https://github.com/whozghiar/jak-level-editor/releases/download/vX.Y.Z/jak-level-editor-portable-<platform>.zip`:
it works once the release is published (a draft's files cannot be downloaded).

A failed platform can be built again: **Re-run failed jobs** on the workflow's run (the assets
already uploaded are replaced).

## 5. The OpenGOAL Launcher

Nothing to do in the launcher: it reads this repository's releases. At its next start, each
launcher compares the latest published release with the editor version it has active, and shows a
download icon in its header when the release is newer. Its *Settings > Level Editor* then downloads
the release's `jak-level-editor-portable-<platform>.zip`, so a release must hold the portable zips
of `windows-x64` and `linux-x64`.

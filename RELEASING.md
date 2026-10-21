# Making a release

A release is a **draft** GitHub release that `.github/workflows/release.yaml` makes from a tag
`vX.Y.Z`, with the editor built for Windows and Linux. You check it, then publish it by hand.

## 1. Bump the version

The same version in three files:

| File | Where |
|---|---|
| `package.json` | `"version"` |
| `Cargo.toml` | `version` of `[workspace.package]` (every crate takes it) |
| `src-tauri/tauri.conf.json` | `"version"` (the installers' version) |

Then run `cargo check -p jak-level-editor-core` (it writes the new version into `Cargo.lock`) and
commit the four files.

## 2. Tag and push

```bash
git tag v2.1.0
git push                  # the commit
git push origin v2.1.0    # the tag
```

The tag starts the workflow (the **Actions** tab, **Release**). Its first step fails when the tag
and the three versions differ: fix the files, then move the tag
(`git tag -f v2.1.0 && git push -f origin v2.1.0`).

Without a tag: **Actions > Release > Run workflow** on a branch. The draft is then named after the
version of `package.json`, and its tag is created on the commit built when you publish it.

## 3. Check the draft

The workflow takes 20 to 30 minutes. The draft (**Releases**, marked *Draft*) then holds, for
`windows-x64` and `linux-x64`:

| Asset | What it is |
|---|---|
| `Jak.Level.Editor_X.Y.Z_x64-setup.exe` | Windows installer (for the current user, no administrator rights) |
| `Jak.Level.Editor_X.Y.Z_x64_en-US.msi` | Windows installer (MSI, for all users) |
| `jak-level-editor_X.Y.Z_amd64.deb`, `Jak.Level.Editor_X.Y.Z_amd64.AppImage` | Linux packages |
| `jak-level-editor-portable-<platform>.zip` | the editor without installing: unzip it anywhere and run `jak-level-editor` |
| `ogle-extract-<platform>.zip` | the extractor alone (`ogle-extract` and `ogle-extract-data/` at its root): the OpenGOAL Launcher downloads it |
| `jak-level-editor-mcp-<platform>.zip` | the MCP server, for an AI assistant (optional): put it next to the editor's program |

Download the portable zip of your system, start the editor, and open **File > Extract game
assets...**: it must find ogle-extract (no red message) and recognize your disc.

Edit the release notes GitHub wrote if you like.

## 4. Publish

**Edit** the draft, then **Publish release**. The OpenGOAL Launcher downloads
`https://github.com/whozghiar/open-goal-level-builder/releases/download/vX.Y.Z/ogle-extract-<platform>.zip`:
it works once the release is published.

A failed platform can be built again: **Re-run failed jobs** on the workflow's run (the assets
already uploaded are replaced).

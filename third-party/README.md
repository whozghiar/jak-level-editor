# Third-party code

The C++ editor's copies (Dear ImGui, glad, tinygltf, nlohmann json, stb, Recast) are gone with it.
What the editor uses of others' code now, and where its licenses are:

| Where | What | License |
|---|---|---|
| `Cargo.toml`, `Cargo.lock` | the Rust crates (Tauri, serde, image, glam, rerecast...), downloaded by cargo | each crate's own (MIT or Apache-2.0 for most) |
| `package.json`, `yarn.lock` | the interface's packages (Svelte, Tailwind CSS, Flowbite, Tauri's API, Material Design Icons), downloaded by yarn | each package's own (MIT for most; the icons: Apache-2.0) |
| `crates/core/src/navmesh/recast.rs` | the steps of Recast ([recastnavigation](https://github.com/recastnavigation/recastnavigation), commit 9f4ce64) where its Rust port rerecast differs, ported | zlib (in the file's header) |
| `src/assets/fonts/` | Inter, Noto Sans and Noto Sans Mono | SIL Open Font License (`LICENSE.txt`, `OFL.txt` beside them) |

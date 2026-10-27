//! The MCP server's tools on the catalog, prefabs, placing, nav meshes and the export: what
//! waits for the library's workers.

use super::*;

/// the shape presets' names, as the tool says them
pub(super) const SHAPES: &str = "name: box, floor, wall, doorway, window, pillar, ramp, stairs, roof, roof-hip, cylinder or ground";

impl Editor {
  /// `f` now, else asked again until it answers (the library's workers polled in between: the
  /// plugin's timer does it too)
  pub(super) fn now_or_wait(
    &mut self,
    mut f: impl FnMut(&mut Editor) -> Option<Reply> + Send + 'static,
  ) -> Reply {
    match f(self) {
      Some(r) => r,
      None => Reply::wait(move |ed| {
        ed.poll_library_work();
        f(ed)
      }),
    }
  }

  /// the library's index merged into the catalog (its variants, effects, decor parts)
  pub(super) fn indexed(&self, jak2: bool) -> bool {
    self.catalog_state()[if jak2 { "ready_jak2" } else { "ready" }] == true
  }

  /// Some(the tool asked again once the index is merged) while it is not
  pub(super) fn when_indexed(&mut self, jak2: bool, tool: &str, a: &Value) -> Option<Reply> {
    if self.indexed(jak2) {
      return None;
    }
    let (tool, a) = (tool.to_string(), a.clone());
    Some(self.now_or_wait(move |ed| ed.indexed(jak2).then(|| ed.remote_tool(&tool, &a))))
  }

  pub(super) fn tool_list_catalog(&mut self, a: &Value) -> Reply {
    if let Some(w) = self.when_indexed(false, "list_catalog", a) {
      return w;
    }
    let family = str_arg(a, "family");
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    // the objects that make up a place (props, doors, water, effects), or the interactive ones
    let group = str_arg(a, "group");
    let mut objects = Vec::new();
    let mut matching = 0;
    for o in &self.catalog.objects {
      let effect = o.family == "effect";
      if (!family.is_empty() && o.family != family)
        || (group == "objects" && (o.interactive || effect))
        || (group == "interactive" && !o.interactive)
        || (group == "effects" && !effect)
        || (!f.is_empty() && !o.name.contains(&f) && !o.etype.contains(&f))
      {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || objects.len() >= limit {
        continue;
      }
      let g = if effect {
        "effects"
      } else if o.interactive {
        "interactive"
      } else {
        "objects"
      };
      let mut j = json!({"name": o.name, "etype": o.etype, "family": o.family, "group": g});
      if effect {
        j["effect_kind"] = json!(effect_kind(o));
      }
      if o.model.is_empty() {
        j["drawn_without_model"] = json!(true);
      } else {
        j["model"] = json!(o.model);
      }
      if o.count != 0 {
        j["in_game"] = json!(o.count);
      }
      objects.push(j);
    }
    let mut families: Vec<&str> = Vec::new();
    for o in &self.catalog.objects {
      if families.last() != Some(&o.family.as_str()) {
        families.push(&o.family);
      }
    }
    Ok(json!({"total": matching, "offset": offset, "objects": objects, "families": families}))
      .into()
  }

  pub(super) fn tool_list_models(&mut self, a: &Value) -> Reply {
    if let Some(w) = self.when_indexed(false, "list_models", a) {
      return w;
    }
    // Jak II's objects by their model (game: "jak2"), placed as decor, or Jak 3's models
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let mut models = Vec::new();
    let mut matching = 0;
    let all = if jak2 {
      &self.catalog.models_jak2
    } else {
      &self.catalog.models
    };
    for m in all {
      if !f.is_empty() && !m.name.contains(&f) && !m.group.contains(&f) {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || models.len() >= limit {
        continue;
      }
      models.push(json!({"name": m.name, "from": m.group}));
    }
    Ok(json!({"total": matching, "offset": offset, "models": models})).into()
  }

  pub(super) fn tool_list_decor(&mut self, a: &Value) -> Reply {
    // Jak II's structures (game: "jak2"), brought into Jak 3 levels, or Jak 3's decor
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let name = str_arg(a, "level").to_string();
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let kind = |p: &CatalogPart| {
      if p.kind == PartKind::Prototype {
        "prototype"
      } else {
        "piece"
      }
    };
    if name.is_empty() || name == "all" {
      // the decor of every level: once the editor has read them
      if let Some(w) = self.when_indexed(jak2, "list_decor", a) {
        return w;
      }
      let mut parts = Vec::new();
      let mut matching = 0;
      let all = if jak2 {
        &self.catalog.all_parts_jak2
      } else {
        &self.catalog.all_parts
      };
      for e in all {
        if !f.is_empty() && !e.key.contains(&f) {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || parts.len() >= limit {
          continue;
        }
        let mut j = json!({"name": e.part.name, "level": e.level, "kind": kind(&e.part),
          "triangles": e.part.triangles});
        if e.levels > 1 {
          j["in_levels"] = json!(e.levels);
        }
        parts.push(j);
      }
      return Ok(json!({"total": matching, "offset": offset, "parts": parts})).into();
    }
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    let Some(background) = lib.level(&name).map(|l| l.background.clone()) else {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    };
    self.now_or_wait(move |ed| {
      let Some(scene) = ed.decor_scene_of(&background) else {
        return ed
          .catalog
          .store
          .failed_level(&background)
          .then(|| Err(format!("the decor of {name} cannot be read")).into());
      };
      let structures_only = ed.library_jak2.level_of(&background).is_some();
      let mut parts = Vec::new();
      let mut matching = 0;
      for p in list_parts(&scene) {
        if (!f.is_empty() && !lower(&p.name).contains(&f))
          || (structures_only && !jak2_structure(&p))
        {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || parts.len() >= limit {
          continue;
        }
        let mut j = json!({"name": p.name, "kind": kind(&p), "triangles": p.triangles});
        if p.kind == PartKind::Prototype {
          j["copies_in_level"] = json!(p.copies);
        }
        parts.push(j);
      }
      Some(Ok(json!({"level": name, "total": matching, "offset": offset, "parts": parts})).into())
    })
  }

  pub(super) fn tool_list_prefabs(&mut self, a: &Value) -> Reply {
    self.refresh_prefabs();
    // make: the game's prefabs not made yet made first from the extracted levels (waits); else
    // each is made with its level's when it is placed
    let rebuild = flag(a, "rebuild", false);
    if rebuild || flag(a, "make", false) {
      self.start_game_prefabs(rebuild, &BTreeSet::new());
    }
    let runs = self.catalog.game_prefab_runs;
    let waiting = self.game_prefabs_busy();
    self.now_or_wait(move |ed| {
      if waiting && ed.catalog.game_prefab_runs == runs && ed.game_prefabs_busy() {
        return None;
      }
      let mine: Vec<&str> = ed.catalog.prefabs.iter().map(|p| p.name.as_str()).collect();
      let game: Vec<Value> = ed
        .catalog
        .game_prefabs
        .iter()
        .map(|p| {
          let g = if p.game.is_empty() { "jak3" } else { &p.game };
          json!({"name": p.name, "category": p.category, "description": p.description,
            "game": g, "level": p.level, "made": p.made})
        })
        .collect();
      Some(Ok(json!({"prefabs": mine, "game_prefabs": game})).into())
    })
  }

  pub(super) fn tool_create_prefab(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let name = str_arg(a, "name");
    if ids.is_empty() || name.is_empty() {
      return Err("ids and name".into());
    }
    self.selection = ids
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.fix_primary();
    if self.selection.is_empty() {
      return Err("no element with those ids".into());
    }
    // the small pieces left out (bolts, debris, stains): 0.5 m by default
    let mut left_out = Vec::new();
    let min_size = num(a, "min_size", 0.5).max(0.0);
    let path = self.create_prefab(name, min_size, Some(&mut left_out));
    if path.is_empty() {
      return Err("the prefab could not be saved (see the editor's log)".into());
    }
    // its name (<folder>/<name>.glb): the one asked, or with -2, -3... when a prefab has it already
    let stem = Path::new(&path)
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
    let prefabs: Vec<&str> = self
      .catalog
      .prefabs
      .iter()
      .map(|p| p.name.as_str())
      .collect();
    Ok(json!({"name": stem, "left_out": left_out, "prefabs": prefabs}))
  }

  /// place: what to put by its kind and name, where (remote_place)
  pub(super) fn remote_place(&mut self, a: &Value) -> Reply {
    let fail = |e: String| -> Reply { Err(e).into() };
    let kind = a.get("kind").and_then(Value::as_str).unwrap_or("object");
    let name = str_arg(a, "name");
    let Some(position) = vec3_arg(a, "position") else {
      return fail("position: [x, y, z] in meters".into());
    };
    if !self.document_open() {
      return fail("no document: open_level or new_level first".into());
    }
    let yaw = radians(num(a, "yaw_deg", 0.0));
    let on_ground = flag(a, "on_ground", false);
    // a full rotation (degrees around x, y, z) instead of the yaw, a scale ([x, y, z] or one
    // number)
    let rotation = vec3_arg(a, "rotation_deg");
    let s = match a.get("scale").and_then(Value::as_f64) {
      Some(k) => Vec3::new(k as f32, k as f32, k as f32),
      None => vec3_arg(a, "scale").unwrap_or(Vec3::new(1.0, 1.0, 1.0)),
    };
    let scale = Vec3::new(s.x.max(0.01), s.y.max(0.01), s.z.max(0.01));
    let jak2 = str_arg(a, "game") == "jak2";

    // what to place: by name
    match kind {
      "model" if a.get("path").is_some() => {
        // a model made elsewhere
        let path = str_arg(a, "path");
        let file = Path::new(path);
        if !file.is_file() {
          return fail(format!("path: no file {path}"));
        }
        let stem = file
          .file_stem()
          .map(|s| s.to_string_lossy().into_owned())
          .unwrap_or_default();
        let model = CatalogModel {
          name: if name.is_empty() { stem } else { name.into() },
          path: path.into(),
          ..Default::default()
        };
        self.start_model_placement(&model, false);
      }
      "model" => {
        let models = if jak2 {
          &self.catalog.models_jak2
        } else {
          &self.catalog.models
        };
        let Some(found) = models.iter().rfind(|m| m.name == name).cloned() else {
          return fail(format!(
            "no model '{name}' in the catalog (see list_models)"
          ));
        };
        self.start_model_placement(&found, false);
      }
      "shape" => {
        let Some(preset) = shape_preset(name) else {
          return fail(SHAPES.into());
        };
        // in the style of a level: its floors' (a floor, a ground) or its walls' texture, size
        // and light, its ground's bumps; measured once its decor is read
        let style = str_arg(a, "style");
        if !style.is_empty() {
          let Some(bg) = self.library.level(style).map(|l| l.background.clone()) else {
            return fail(format!("style: no level '{style}' (see list_levels)"));
          };
          if self.level_style(style).is_none() {
            if self.catalog.store.failed_level(&bg) {
              return fail(format!("style: the decor of {style} cannot be read"));
            }
            let (a, style) = (a.clone(), style.to_string());
            return self.now_or_wait(move |ed| {
              (ed.level_style(&style).is_some() || ed.catalog.store.failed_level(&bg))
                .then(|| ed.remote_place(&a))
            });
          }
          // a building's walls', a roof's or a frame's look: the level's buildings' (their kit)
          let surface = str_arg(a, "surface");
          if surface == "building" || surface == "roof" || surface == "trim" {
            let Some(kit) = self.building_kit(style).cloned() else {
              let (a, style) = (a.clone(), style.to_string());
              return self.now_or_wait(move |ed| {
                (ed.building_kit(&style).is_some() || ed.kit_failed(&style))
                  .then(|| ed.remote_place(&a))
              });
            };
            let s = match surface {
              "building" => &kit.wall,
              "roof" => &kit.roof,
              _ => &kit.trim,
            };
            let material = self.decor_material(style, s.material);
            if material < 0 {
              return fail(format!("surface: {style} has no {surface} texture"));
            }
            let mut preset = preset.clone();
            preset.params.tile = s.tile;
            preset.params.color = s.color;
            self.start_shape_placement(&preset, false);
            self.placement_mut().material = material;
          } else {
            let floor = match surface {
              "floor" => true,
              "wall" => false,
              _ => name == "floor" || name == "ground",
            };
            self.start_style_placement(style, floor, name);
          }
        }
        let mut j = if style.is_empty() {
          preset.params.to_json()
        } else {
          self.placement().shape.to_json()
        };
        for k in [
          "size",
          "tile",
          "steps",
          "sides",
          "opening",
          "bend",
          "round",
          "bumps",
          "bump_size",
          "seed",
        ] {
          if let Some(v) = a.get(k) {
            j[k] = v.clone();
          }
        }
        let mut p = preset.clone();
        p.params = ShapeParams::from_json(&j);
        let material = if style.is_empty() {
          -1
        } else {
          self.placement().material
        };
        self.start_shape_placement(&p, false);
        self.placement_mut().material = material;
      }
      "object" => {
        let objects = &self.catalog.objects;
        let Some(found) = objects
          .iter()
          .rfind(|o| o.name == name)
          .or_else(|| objects.iter().find(|o| o.etype == name))
          .cloned()
        else {
          return fail(format!(
            "no object '{name}' in the catalog (see list_catalog)"
          ));
        };
        self.start_object_placement(&found, false);
      }
      "prefab" => {
        self.refresh_prefabs();
        let c = &self.catalog;
        let Some(found) = c
          .prefabs
          .iter()
          .chain(&c.game_prefabs)
          .rfind(|p| p.name == name)
          .cloned()
        else {
          return fail(format!("no prefab '{name}' (see list_prefabs)"));
        };
        if !found.made {
          // a game's prefab not made yet: made with its level's, then placed
          let runs = self.catalog.game_prefab_runs;
          self.want_game_prefabs(&found.game, &found.level);
          let a = a.clone();
          return Reply::wait(move |ed| {
            ed.poll_library_work();
            if ed.catalog.game_prefab_runs == runs || ed.game_prefabs_busy() {
              return None;
            }
            let made = ed
              .catalog
              .game_prefabs
              .iter()
              .any(|p| p.name == found.name && p.made);
            Some(if made {
              ed.remote_place(&a)
            } else {
              Err("the prefab could not be made from its level (see the editor's log)".to_string())
                .into()
            })
          });
        }
        self.start_placement(&found.path, &found.name, false);
      }
      "decor" => {
        // a structure of Jak II (game: "jak2"): converted for Jak 3 (io/jak2_import)
        let mut level_name = str_arg(a, "level").to_string();
        let mb = format!("{name}.mb");
        if level_name.is_empty() {
          // the first level with a part of that name
          if let Some(w) = self.when_indexed(jak2, "place", a) {
            return w;
          }
          let all = if jak2 {
            &self.catalog.all_parts_jak2
          } else {
            &self.catalog.all_parts
          };
          let Some(e) = all
            .iter()
            .find(|e| e.part.name == name || e.part.name == mb)
          else {
            return fail(format!(
              "no decor part '{name}' in the levels (see list_decor)"
            ));
          };
          level_name = e.level.clone();
        }
        let lib = if jak2 {
          &self.library_jak2
        } else {
          &self.library
        };
        let Some(level) = lib.level(&level_name).cloned() else {
          return fail("level: the level the decor part comes from (see list_decor)".into());
        };
        let Some(source) = self.decor_scene_of(&level.background) else {
          if self.catalog.store.failed_level(&level.background) {
            return fail("the decor of that level cannot be read".into());
          }
          // loading: tried again
          let a = a.clone();
          return self.now_or_wait(move |ed| {
            let bg = &level.background;
            (ed.decor_scene_of(bg).is_some() || ed.catalog.store.failed_level(bg))
              .then(|| ed.remote_place(&a))
          });
        };
        let parts = list_parts(&source);
        let Some(found) = parts.iter().rfind(|p| p.name == name || p.name == mb) else {
          return fail(format!(
            "no decor part '{name}' in {} (see list_decor)",
            level.name
          ));
        };
        self.start_part_placement(&level.background, found, false);
      }
      _ => return fail("kind: object, model, decor, prefab or shape".into()),
    }
    self.placement_mut().yaw = yaw;

    // where: on the surface below, when asked
    let mut p = position;
    if on_ground {
      let f = PickFilter {
        actors: false,
        include_locked: true,
        ..Default::default()
      };
      let ray = Ray::new(p + Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
      let hit = pick(&self.scene, &ray, &f);
      if hit.hit() {
        p = hit.point;
      }
    }
    let rot = rotation.map_or(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), yaw), |r| {
      Quat::from_euler_deg(r)
    });
    let m = Mat4::translate(p) * Mat4::rotate(rot) * Mat4::scale(scale);
    let prefab = kind == "prefab";
    // its model or its prefab may still be loading
    self.now_or_wait(move |ed| {
      let pl = ed.placement();
      let markerless = pl.kind == PlacementKind::Object && pl.path.is_empty();
      if !prefab && !markerless && ed.placement_mesh().is_none() {
        if !ed.placement_failed() {
          return None;
        }
        ed.stop_placement();
        return Some(Err("its model cannot be read".to_string()).into());
      }
      let roots = ed.place_at(&m);
      ed.stop_placement();
      if roots.is_empty() {
        return Some(Err("nothing was placed".to_string()).into());
      }
      let nodes: Vec<Value> = roots.iter().map(|&id| ed.node_json(id)).collect();
      Some(Ok(json!({ "nodes": nodes })).into())
    })
  }

  pub(super) fn tool_place_many(&mut self, a: &Value) -> Reply {
    let Some(items) = a
      .get("items")
      .and_then(Value::as_array)
      .filter(|l| !l.is_empty())
      .cloned()
    else {
      return Err("items: a list of placements".to_string()).into();
    };
    if items.len() > 2000 {
      return Err("items: at most 2000 at once".to_string()).into();
    }
    if !self.document_open() {
      return Err("no document: open_level or new_level first".to_string()).into();
    }
    // one undo entry for the whole batch, lit like the level around it at its end
    self.begin_place_batch();
    let mut placed: Vec<Value> = Vec::new();
    let mut next = 0;
    let mut current: Option<Reply> = None;
    self.now_or_wait(move |ed| {
      loop {
        let reply = match current.take() {
          Some(r) => r,
          None if next < items.len() => ed.remote_place(&items[next]),
          None => {
            ed.end_place_batch();
            let count = placed.iter().filter(|p| p.get("id").is_some()).count();
            let nodes = std::mem::take(&mut placed);
            return Some(Ok(json!({"placed": count, "nodes": nodes})).into());
          }
        };
        match reply {
          Reply::Now(answer) => {
            if answer["ok"] == true {
              placed.extend(
                answer["result"]["nodes"]
                  .as_array()
                  .cloned()
                  .unwrap_or_default(),
              );
            } else {
              placed.push(json!({"index": next, "error": answer["error"]}));
            }
            next += 1;
          }
          Reply::Wait(mut w) => match w(ed) {
            Some(r) => current = Some(r),
            None => {
              current = Some(Reply::Wait(w));
              return None;
            }
          },
        }
      }
    })
  }

  /// A house in the style of a level (editor/structures), at `position` (dropped onto the surface
  /// below unless on_ground is false), its front towards `yaw_deg`
  pub(super) fn tool_build_house(&mut self, a: &Value) -> Reply {
    let mut a = a.clone();
    a["kind"] = "house".into();
    self.tool_build_structure(&a)
  }

  /// A wall, a floor or a house in the style of a level (editor/structures), at `position`
  /// (dropped onto the surface below unless on_ground is false), its front towards `yaw_deg`
  pub(super) fn tool_build_structure(&mut self, a: &Value) -> Reply {
    let o = StructureParams::from_json(a);
    if !K_STRUCTURE_KINDS.contains(&str_arg(a, "kind")) {
      return Err("kind: wall, floor or house".to_string()).into();
    }
    let Some(position) = vec3_arg(a, "position") else {
      return Err("position: [x, y, z] in meters".to_string()).into();
    };
    if !self.document_open() {
      return Err("no document: open_level or new_level first".to_string()).into();
    }
    if self.library.level(&o.level).is_none() {
      return Err(format!("level: no level '{}' (see list_levels)", o.level)).into();
    }
    let mut p = position;
    if flag(a, "on_ground", true) {
      // on the highest ground under it (its foundation goes down where it is lower)
      let f = PickFilter {
        actors: false,
        include_locked: true,
        ..Default::default()
      };
      let mut top: Option<f32> = None;
      for (dx, dz) in [(0.0, 0.0), (8.0, 0.0), (-8.0, 0.0), (0.0, 8.0), (0.0, -8.0)] {
        let from = p + Vec3::new(dx, 6.0, dz);
        let hit = pick(&self.scene, &Ray::new(from, Vec3::new(0.0, -1.0, 0.0)), &f);
        if hit.hit() {
          top = Some(top.map_or(hit.point.y, |y: f32| y.max(hit.point.y)));
        }
      }
      if let Some(y) = top {
        p.y = y;
      }
    }
    let m = Mat4::translate(p)
      * Mat4::rotate(Quat::axis_angle(
        Vec3::new(0.0, 1.0, 0.0),
        radians(num(a, "yaw_deg", 0.0)),
      ));
    self.now_or_wait(move |ed| match ed.build_structure(&o, &m) {
      Err(e) if e == "reading" => None,
      Err(e) => Some(Err(e).into()),
      Ok((roots, plan)) => {
        let s = plan.size;
        Some(
          Ok(json!({
            "group": format!("{}-{}", o.kind, o.level),
            "elements": roots.len(),
            "size": [s.x, s.y, s.z],
            "parts": plan.used,
            "ids": roots,
          }))
          .into(),
        )
      }
    })
  }

  /// The structure `id` is part of made again at a new size or look (more of the level's pieces,
  /// not stretched)
  pub(super) fn tool_set_structure(&mut self, a: &Value) -> Reply {
    let id = int(a, "id", 0) as u32;
    if self.structure_of(id).is_none() {
      return Err("id: an element of a structure (build_structure, build_house)".to_string())
        .into();
    }
    let mut args = a.clone();
    for (from, to) in [("length", 0), ("width", 0), ("height", 1), ("depth", 2)] {
      if let Some(v) = a.get(from) {
        let (_, p, _) = self.structure_of(id).unwrap();
        let mut size = p.to_json()["size"].clone();
        if args.get("size").is_some() {
          size = args["size"].clone();
        }
        size[to] = v.clone();
        args["size"] = size;
      }
    }
    self.now_or_wait(move |ed| match ed.structures_call("set_structure", &args) {
      Some(Ok(v)) if v.get("reading").is_some() => None,
      Some(Ok(v)) => Some(
        Ok(
          json!({"elements": v["roots"].as_array().map_or(0, Vec::len),
        "ids": v["roots"], "structure": ed.structure_info(id_of(&v))}),
        )
        .into(),
      ),
      Some(Err(e)) => Some(Err(e).into()),
      None => Some(Err("not a structure".to_string()).into()),
    })
  }

  /// A set of prefabs of a level's structures, saved with the user's prefabs
  pub(super) fn tool_create_prefab_set(&mut self, a: &Value) -> Reply {
    let level = str_arg(a, "level").to_string();
    if self.library.level(&level).is_none() {
      return Err(format!("level: no level '{level}' (see list_levels)")).into();
    }
    self.now_or_wait(move |ed| match ed.create_prefab_set(&level) {
      Err(e) if e == "reading" => None,
      Err(e) => Some(Err(e).into()),
      Ok(paths) => {
        let names: Vec<String> = paths
          .iter()
          .map(|p| {
            Path::new(p)
              .file_stem()
              .map(|s| s.to_string_lossy().into_owned())
              .unwrap_or_default()
          })
          .collect();
        Some(Ok(json!({"prefabs": names})).into())
      }
    })
  }

  /// What a level's buildings are made of: its parts by role (their size once turned to face +z,
  /// the rotation that does it), its walls', roofs' and frames' textures
  pub(super) fn tool_building_kit(&mut self, a: &Value) -> Reply {
    let level = str_arg(a, "level").to_string();
    if self.kit_background(&level).is_none() {
      return Err(format!("level: no level '{level}' (see list_levels)")).into();
    }
    self.now_or_wait(
      move |ed| match ed.building_kit(&level).map(|k| k.to_json()) {
        Some(k) => Some(Ok(json!({"level": level, "kit": k})).into()),
        None if ed.kit_failed(&level) => {
          Some(Err(format!("the decor of {level} cannot be read")).into())
        }
        None => None,
      },
    )
  }

  pub(super) fn tool_generate_navmesh(&mut self, a: &Value) -> Reply {
    // the parameters given, the others as the Nav meshes window has them
    let mut params = Map::new();
    for k in [
      "cell_size",
      "agent_radius",
      "agent_height",
      "max_climb",
      "max_slope",
      "max_edge",
      "max_error",
      "min_region",
    ] {
      if let Some(v) = a.get(k).filter(|v| v.is_number()) {
        params.insert(k.into(), v.clone());
      }
    }
    let mut call = json!({ "params": params });
    for k in ["region", "from"] {
      if let Some(v) = a.get(k) {
        call[k] = v.clone();
      }
    }
    let generation = self.nav.generation;
    if let Err(e) = self.generate_navmesh_call(&call) {
      return Err(e).into();
    }
    self.now_or_wait(move |ed| {
      if ed.nav.generation == generation {
        return None;
      }
      let r = match &ed.nav.last {
        Some(Ok(s)) => {
          // the new meshes are the selection
          let meshes: Vec<Value> = ed.selection.iter().map(|&id| ed.node_json(id)).collect();
          Ok(json!({
            "meshes": meshes,
            "polygons": s.polys,
            "pieces_split": s.split,
            "pieces_left_out": s.left_out,
            "triangles_read": s.triangles,
            "grid": [s.grid_width, s.grid_depth],
            "seconds": (s.ms / 100.0).round() / 10.0,
          }))
        }
        Some(Err(e)) => Err(format!("nav mesh not generated: {e}")),
        None => Err("nav mesh not generated".to_string()),
      };
      Some(r.into())
    })
  }

  pub(super) fn tool_export_level(&mut self, a: &Value) -> Reply {
    if !self.document_open() {
      return Err("no level open: open_level or open_project first".to_string()).into();
    }
    let mut o = self.default_export_options();
    o.name = str_arg(a, "name").to_string();
    // the default nickname is one no level of the game has
    let data = self.document_game_data();
    o.nickname = match a.get("nickname").and_then(Value::as_str) {
      Some(n) => n.to_string(),
      None => untaken_nickname(&default_nickname(&o.name), data.as_deref()),
    };
    o.iso_name = match a.get("iso_name").and_then(Value::as_str) {
      Some(n) => n.to_string(),
      None => default_iso_name(&o.name),
    };
    o.replaces = match a.get("replaces").and_then(Value::as_str) {
      Some(r) => r.to_string(),
      None => self.scene.settings.replaces.clone(),
    };
    if let Some(id) = a.get("base_id").and_then(Value::as_u64) {
      o.base_id = id as u32;
    }
    if let Some(f) = a.get("folder").and_then(Value::as_str) {
      o.folder = f.to_string();
    }
    o.actors = flag(a, "actors", true);
    let collision = a.get("collision").and_then(Value::as_str).unwrap_or("game");
    if collision != "game" && collision != "decor" {
      return Err("collision: 'game' or 'decor'".to_string()).into();
    }
    o.game_collision = collision == "game";
    if let Some(s) = vec3_arg(a, "start") {
      o.start = s;
    }
    if let Some(bad) = self.export_check(&o) {
      return Err(tr_en(bad).to_string()).into();
    }
    let start = o.start;
    let generation = self.export.generation;
    if let Err(e) = self.export_for_game(o, false) {
      return Err(e).into();
    }
    self.now_or_wait(move |ed| {
      if ed.export.generation == generation {
        return None;
      }
      let Some(r) = ed
        .export
        .report
        .as_ref()
        .filter(|_| ed.export.error.is_empty())
      else {
        return Some(Err(ed.export.error.clone()).into());
      };
      let notes: Vec<Value> = r
        .notes
        .iter()
        .map(|n| {
          json!({"key": n.key, "count": n.count, "detail": n.detail,
            "text": strf!(tr_en(&n.key), n.count, &n.detail)})
        })
        .collect();
      let dense: Vec<Value> = r
        .densest
        .iter()
        .map(|&(n, at)| json!({"triangles": n, "position": v3(at)}))
        .collect();
      Some(
        Ok(json!({
          "folder": r.dir,
          "dense_places": r.dense_places,
          "densest": dense,
          "level_folder": r.level_dir,
          "files": r.files,
          "meshes": r.meshes,
          "triangles": r.triangles,
          "collision_triangles": r.collision_triangles,
          "meshes_colliding_by_shape": r.collision_from_decor,
          "actors": r.actors,
          "art_groups": r.art_groups,
          "navmeshes": r.navmeshes,
          "start": v3(start),
          "notes": notes,
        }))
        .into(),
      )
    })
  }
}

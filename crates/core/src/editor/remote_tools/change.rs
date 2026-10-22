//! The MCP server's tools that change things: shapes, materials, actors, visibility, lighting,
//! decor, nav meshes, moves, copies, deletions, groups, the selection.

use super::*;

impl Editor {
  pub(super) fn tool_set_shape(&mut self, a: &Value) -> R {
    let id = uint(a, "id");
    let p = self
      .scene
      .find(id)
      .and_then(node_shape)
      .ok_or("id: a shape (place kind 'shape')")?;
    let mut j = p.to_json();
    for k in ["size", "tile", "steps", "sides", "opening", "bend", "round"] {
      if let Some(v) = a.get(k) {
        j[k] = v.clone();
      }
    }
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, id);
    reshape(&mut self.scene, id, &ShapeParams::from_json(&j));
    self
      .history
      .push(c.commit(&self.scene, tr("undo.shape"), None));
    Ok(self.node_json(id))
  }

  pub(super) fn tool_set_material(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: the elements".into());
    }
    let tex = str_arg(a, "texture");
    if tex.is_empty() {
      return Err("texture: an image file, or a texture name (list_textures)".into());
    }
    let mut texture = self
      .scene
      .textures
      .iter()
      .position(|t| t.name == tex)
      .map_or(-1, |i| i as i32);
    // a texture of the games' levels (Jak II's named jak2-<name>), read now
    let c = &self.catalog;
    let games = if c.game_textures.is_empty() {
      &c.palette_textures
    } else {
      &c.game_textures
    };
    let found = games.iter().find(|t| t.name == tex).or_else(|| {
      c.game_textures_jak2
        .iter()
        .find(|t| format!("jak2-{}", t.name) == tex)
    });
    if texture < 0
      && let Some(t) = found.cloned()
    {
      texture = self.palette_texture(&t, true);
    }
    let texture = match texture {
      i if i >= 0 => i,
      _ => texture_from_image(&mut self.scene, tex).map_err(|e| {
        format!(
          "texture: no texture '{tex}' in the document or the games' levels (list_textures), nor \
           an image file ({e})"
        )
      })?,
    };
    let mut c = NodeChange::new(&self.scene);
    for &id in &ids {
      c.save(&self.scene, id);
    }
    let material = int(a, "material", -1) as i32;
    let changed = edit_material(&mut self.scene, &ids, material, |m| m.texture = texture);
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.texture"), None));
    }
    Ok(json!({ "changed": changed }))
  }

  pub(super) fn tool_list_textures(&mut self, a: &Value) -> R {
    let f = lower(str_arg(a, "filter"));
    let offset = int(a, "offset", 0).max(0) as usize;
    let limit = int(a, "limit", 100).clamp(1, 1000) as usize;
    let mut list = Vec::new();
    let mut matching = 0;
    if flag(a, "palette", false) {
      // the textures Jak 3's levels use again and again, the most shared first
      for t in &self.catalog.palette_textures {
        if !f.is_empty() && !lower(&t.name).contains(&f) && t.theme != f {
          continue;
        }
        matching += 1;
        if matching - 1 < offset || list.len() >= limit {
          continue;
        }
        list.push(json!({"name": t.name, "theme": t.theme, "levels": t.levels, "pieces": t.uses}));
      }
      return Ok(json!({"total": matching, "offset": offset, "textures": list}));
    }
    for t in &self.scene.textures {
      if !f.is_empty() && !lower(&t.name).contains(&f) {
        continue;
      }
      matching += 1;
      if matching - 1 < offset || list.len() >= limit {
        continue;
      }
      list.push(json!({"name": t.name, "size": [t.width, t.height]}));
    }
    Ok(json!({"total": matching, "offset": offset, "textures": list}))
  }

  pub(super) fn tool_set_actor(&mut self, a: &Value) -> R {
    // an actor's behavior (merged into og_behavior) and parameters (its lumps, typed by its
    // type's code)
    let id = uint(a, "id");
    let extras = self
      .scene
      .find(id)
      .filter(|n| n.kind == NodeKind::Actor)
      .map(|n| n.extras.clone())
      .ok_or("id: an actor (find_nodes kind 'actor')")?;
    let object = |k: &str| {
      extras
        .get(k)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
    };
    let (mut b, mut lump, mut types) =
      (object("og_behavior"), object("lump"), object("lump_types"));
    let etype = str_arg(&extras, "etype");
    let data = self.game_data();
    let ps = data.as_ref().and_then(|d| d.params(etype));
    if let Some(behavior) = a.get("behavior") {
      let behavior = behavior
        .as_object()
        .ok_or("behavior: an object (see the tool's description)")?;
      // checked here: a value of the wrong kind would stop the inspector and the export
      for (k, v) in behavior {
        if v.is_null() {
          continue;
        }
        let condition = v.get("when").is_some_and(Value::is_string);
        if k == "open"
          && !v
            .as_str()
            .is_some_and(|s| ["approach", "locked", "always", "when"].contains(&s))
        {
          return Err(
            "behavior.open: \"approach\", \"locked\", \"always\" or \"when\" (with \"when\": a \
             condition, beside it: {\"open\": \"when\", \"when\": {\"when\": \"gone\", \"arg\": \
             \"crate-1\"}, \"distance\": 10})"
              .into(),
          );
        }
        if ["when", "active", "leave", "present"].contains(&k.as_str()) && !condition {
          return Err(format!(
            "behavior.{k}: a condition {{\"when\": always|never|task|pressed|gone, \"arg\": ...}}"
          ));
        }
        if k == "distance" && !v.is_number() {
          return Err("behavior.distance: meters".into());
        }
        if k == "loads" && !v.is_string() {
          return Err("behavior.loads: a level's name".into());
        }
      }
      for (k, v) in behavior {
        if v.is_null() {
          b.remove(k);
        } else {
          b.insert(k.clone(), v.clone());
        }
      }
    }
    let mut unknown = Vec::new();
    if let Some(params) = a.get("params") {
      let params = params
        .as_object()
        .ok_or("params: an object, tag -> value (get_node: params)")?;
      for (tag, v) in params {
        let Some(p) = ps.and_then(|ps| ps.iter().rfind(|q| &q.tag == tag)) else {
          unknown.push(json!(tag));
          continue;
        };
        if v.is_null() {
          lump.remove(tag);
          types.remove(tag);
          continue;
        }
        let mut value = v.clone();
        let ty = match p.kind.as_str() {
          "ints" | "eco-info" | "int" => "int32",
          k if k.starts_with("enum:") => "uint32",
          "vector" => "vector",
          "string" => "string",
          "actor" => "actor-name",
          "symbol" => {
            if let Some(s) = v.as_str().filter(|s| !s.starts_with('\'')) {
              value = json!(format!("'{s}"));
            }
            "symbol"
          }
          "float" | "floats" => "float",
          kind => {
            unknown.push(json!(format!("{tag} ({kind}: not settable)")));
            continue;
          }
        };
        lump.insert(tag.clone(), value);
        types.insert(tag.clone(), json!(ty));
      }
    }
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, id);
    let n = self.scene.find_mut(id).unwrap();
    if !n.extras.is_object() {
      n.extras = json!({});
    }
    let extras = n.extras.as_object_mut().unwrap();
    if b.is_empty() {
      extras.remove("og_behavior");
    } else {
      extras.insert("og_behavior".into(), json!(b));
    }
    extras.insert("lump".into(), json!(lump));
    extras.insert("lump_types".into(), json!(types));
    self
      .history
      .push(c.commit(&self.scene, tr("undo.behavior"), None));
    let mut out = json!({"behavior": b, "lump": lump});
    if !unknown.is_empty() {
      out["not_set"] = json!(unknown);
    }
    Ok(out)
  }

  pub(super) fn tool_set_visibility(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let visible = a.get("visible").and_then(Value::as_bool);
    let (false, Some(visible)) = (ids.is_empty(), visible) else {
      return Err("ids and visible (true or false)".into());
    };
    let mut c = NodeChange::new(&self.scene);
    let mut changed = 0;
    for id in ids {
      if self.scene.find(id).is_some_and(|n| n.visible != visible) {
        c.save(&self.scene, id);
        self.scene.find_mut(id).unwrap().visible = visible;
        changed += 1;
      }
    }
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.visibility"), None));
      // a level's group shown or hidden: the ocean follows
      self.update_ocean();
    }
    Ok(json!({ "changed": changed }))
  }

  pub(super) fn tool_match_lighting(&mut self, a: &Value) -> R {
    // the inspector's Match the lighting: the vertex colors of the pieces (and those under them)
    // scaled to the brightness of the document's Jak 3 decor, or by `factor`
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: the elements".into());
    }
    let factor = num(a, "factor", 0.0);
    let mut all = ids.clone();
    all.extend(self.scene.descendants_of(&ids).into_iter().flatten());
    let mut c = NodeChange::new(&self.scene);
    let mut changed = 0;
    for id in all {
      let Some(mesh) = self
        .scene
        .find(id)
        .filter(|n| n.kind == NodeKind::Render)
        .and_then(|n| n.mesh.clone())
        .filter(|m| mesh_lighting(m) != 0.0)
      else {
        continue;
      };
      let k = if factor > 0.0 {
        factor
      } else {
        1.0 / lighting_ratio(&mesh, &self.scene)
      };
      if k <= 0.0 || (k - 1.0).abs() < 1e-3 {
        continue;
      }
      c.save(&self.scene, id);
      self.scene.find_mut(id).unwrap().mesh = Some(scaled_colors(&mesh, k));
      changed += 1;
    }
    if changed > 0 {
      self
        .history
        .push(c.commit(&self.scene, tr("undo.match_lighting"), None));
    }
    Ok(json!({ "changed": changed }))
  }

  pub(super) fn tool_set_decor(&mut self, a: &Value) -> R {
    // what the game does with elements of the decor: their collision, bounce, launch; a part
    // given false goes back to the game's
    let ids = ids_arg(a, "ids");
    if ids.is_empty() {
      return Err("ids: elements of the decor".into());
    }
    let mut c = NodeChange::new(&self.scene);
    let mut changed = Vec::new();
    for id in ids {
      let Some(n) = self.scene.find(id) else {
        continue;
      };
      if n.kind != NodeKind::Render || n.mesh.is_none() || self.scene.actor_of(n) != 0 {
        continue;
      }
      c.save(&self.scene, id);
      let mut d = n
        .extras
        .get("og_decor")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
      for part in ["collision", "bounce", "launch"] {
        match a.get(part) {
          Some(v) if v.is_object() => {
            d.insert(part.into(), v.clone());
          }
          Some(_) => {
            d.remove(part);
          }
          None => {}
        }
      }
      // read back as the editor keeps them (names checked, values clamped)
      let clean = decor_behavior_json(&decor_behavior_of(&json!({ "og_decor": d })));
      let n = self.scene.find_mut(id).unwrap();
      if !n.extras.is_object() {
        n.extras = json!({});
      }
      let extras = n.extras.as_object_mut().unwrap();
      if clean.is_null() {
        extras.remove("og_decor");
      } else {
        extras.insert("og_decor".into(), clean.clone());
      }
      changed.push(json!({"id": id, "decor": clean}));
    }
    if changed.is_empty() {
      return Err("ids: elements of the decor (not actors)".into());
    }
    self
      .history
      .push(c.commit(&self.scene, tr("undo.decor"), None));
    Ok(json!({ "nodes": changed }))
  }

  pub(super) fn tool_export_navmesh(&mut self, a: &Value) -> R {
    let path = str_arg(a, "path");
    if path.is_empty() {
      return Err("path: the nav_data file to write (.json)".into());
    }
    let first = a
      .get("first_id")
      .and_then(Value::as_u64)
      .map_or(self.nav.first_aid, |v| v as u32);
    export_navmeshes(&self.scene, Path::new(path), first)?;
    Ok(json!({"path": path, "meshes": navmesh_nodes(&self.scene).len(), "first_id": first}))
  }

  pub(super) fn tool_transform_nodes(&mut self, a: &Value) -> R {
    let ids = ids_arg(a, "ids");
    let position = vec3_arg(a, "position");
    let translate = vec3_arg(a, "translate");
    let rotation = vec3_arg(a, "rotation_deg");
    let turn = num(a, "rotate_y_deg", 0.0);
    // scale: [x, y, z] or one number for the three; scale_by multiplies the scale they have
    let scale_arg = |k: &str| {
      a.get(k).and_then(|v| {
        v.as_f64()
          .map(|k| Vec3::new(k as f32, k as f32, k as f32))
          .or_else(|| vec3_of(v))
      })
    };
    let (scale, scale_by) = (scale_arg("scale"), scale_arg("scale_by"));
    if ids.is_empty() {
      return Err("ids: the elements to move".into());
    }
    let mut c = NodeChange::new(&self.scene);
    let mut done = Vec::new();
    for &id in &ids {
      if self.scene.find(id).is_none() {
        continue;
      }
      c.save(&self.scene, id);
      if let Some(r) = rotation {
        self.scene.find_mut(id).unwrap().local.r = Quat::from_euler_deg(r);
      }
      let mut w = self.scene.world_of(id);
      if turn != 0.0 {
        let p = w.col3(3);
        let up = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(turn));
        w = Mat4::translate(p) * Mat4::rotate(up) * Mat4::translate(-p) * w;
      }
      if let Some(p) = position {
        w = Mat4::translate(p - w.col3(3)) * w;
      }
      if let Some(t) = translate {
        w = Mat4::translate(t) * w;
      }
      self.scene.set_world(id, &w);
      let n = self.scene.find_mut(id).unwrap();
      if let Some(s) = scale {
        n.local.s = s;
      }
      if let Some(k) = scale_by {
        n.local.s = Vec3::new(n.local.s.x * k.x, n.local.s.y * k.y, n.local.s.z * k.z);
      }
      let s = n.local.s;
      n.local.s = Vec3::new(s.x.max(0.01), s.y.max(0.01), s.z.max(0.01));
      done.push(id);
    }
    let moved: Vec<Value> = done.iter().map(|&id| self.node_json(id)).collect();
    if !done.is_empty() {
      self.relight_moved(&mut c, &done);
      let label = if scale.is_some() || scale_by.is_some() {
        "undo.scale"
      } else if rotation.is_some() || turn != 0.0 {
        "undo.rotate"
      } else {
        "undo.move"
      };
      self.history.push(c.commit(&self.scene, tr(label), None));
    }
    Ok(json!({ "nodes": moved }))
  }

  pub(super) fn tool_delete_nodes(&mut self, a: &Value) -> R {
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      return Err("ids: the elements to delete".into());
    }
    let mut c = NodeChange::new(&self.scene);
    c.save_subtrees(&self.scene, &ids);
    self.scene.remove(&ids);
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.delete"), ids.len()), None));
    let gone: Vec<u32> = self
      .selection
      .iter()
      .copied()
      .filter(|&id| self.scene.find(id).is_none())
      .collect();
    for id in gone {
      self.selection.remove(&id);
    }
    self.fix_primary();
    Ok(json!({ "deleted": ids.len() }))
  }

  pub(super) fn tool_duplicate_nodes(&mut self, a: &Value) -> R {
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      return Err("ids: the elements to copy".into());
    }
    let offset = vec3_arg(a, "offset").unwrap_or_default();
    let c = NodeChange::new(&self.scene);
    let clip = copy_nodes(&self.scene, &ids);
    let copies = paste_nodes(&mut self.scene, &clip);
    for &id in &copies {
      let w = Mat4::translate(offset) * self.scene.world_of(id);
      self.scene.set_world(id, &w);
    }
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.duplicate"), copies.len()), None));
    let list: Vec<Value> = copies.iter().map(|&id| self.node_json(id)).collect();
    Ok(json!({ "nodes": list }))
  }

  pub(super) fn tool_group(&mut self, a: &Value) -> R {
    let name = str_arg(a, "name");
    if name.is_empty() {
      return Err("name: the group's".into());
    }
    let ids: Vec<u32> = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    if ids.is_empty() {
      self.selection.clear();
      self.fix_primary();
      return Err("ids: the elements to group".into());
    }
    self.selection = ids.iter().copied().collect();
    self.fix_primary();
    let gid = self.group_selection(name);
    Ok(json!({"group": gid, "name": name, "elements": self.selection_roots().len()}))
  }

  pub(super) fn tool_select(&mut self, a: &Value) -> R {
    if a.get("group").is_some() {
      let gid = uint(a, "group");
      if self.group_info(gid).is_none() {
        return Err("group: a group's id (list_groups, or 'group' of an element)".into());
      }
      self.select_group(gid);
      return Ok(self.selection_json());
    }
    if let Some(level) = a.get("level") {
      // a whole level: its group, or the root elements of the level opened
      let group = self
        .document_level_group(level, &[LevelRole::Main, LevelRole::Added])
        .ok_or("level: the level opened or a level added (get_status: document.levels)")?;
      self.select_level(group);
      return Ok(self.selection_json());
    }
    if a.get("assembly").is_some() {
      let id = uint(a, "assembly");
      if self.scene.find(id).is_none() {
        return Err("assembly: an element's id".into());
      }
      self.select_assembly(id);
      return Ok(self.selection_json());
    }
    self.selection = ids_arg(a, "ids")
      .into_iter()
      .filter(|&id| self.scene.find(id).is_some())
      .collect();
    self.primary = self.selection.first().copied().unwrap_or(0);
    Ok(self.selection_json())
  }
}

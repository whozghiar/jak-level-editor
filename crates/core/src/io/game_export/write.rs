//! The export itself: the level's files written (see the module's doc).

use super::*;

/// The pieces of a level's decor still where the game has them: its collision holds for them.
pub(super) struct Pieces {
  /// compared with the level as extracted
  known: bool,
  kept: HashSet<String>,
  /// 4 m cells: searched 8 m around
  kept_tris: TriGrid,
  removed_tris: TriGrid,
  removed: usize,
}

pub(super) struct Exported {
  index: usize,
  ty: String,
  name: String,
}

/// Writes the level (see the module's doc). `data`: the game's data (None: the actor types are not
/// checked). The scene's cache must be the current one (Scene::begin_frame after the last changes).
/// The error is a text to show (a translation, or "cannot write <file>").
pub fn export_level(
  scene: &Scene,
  data: Option<&GameData>,
  o: &LevelExportOptions,
) -> Result<LevelExportReport, String> {
  let mut rep = LevelExportReport::default();
  if let Some(bad) = check_export_names(o, data) {
    return Err(tr(bad).to_string());
  }
  // a level replacing one of the game's: the document's own level alone (the levels added next to
  // it are the game's, loaded by the game as they are); never a level shown for reference
  let replacing = !o.replaces.is_empty();
  let replaced = if replacing {
    data.and_then(|d| d.level(&o.replaces))
  } else {
    None
  };
  // the export's folder, laid out like the jak-project checkout it is copied into
  let install_dir = format!("custom_assets/jak3/levels/{}/", o.name);
  let code_dir = format!("goal_src/jak3/levels/{}/", o.name);
  let top = PathBuf::from(&o.folder);
  let dir = top
    .join("custom_assets")
    .join("jak3")
    .join("levels")
    .join(&o.name);
  let _ = std::fs::create_dir_all(&dir);
  if !dir.is_dir() {
    return Err(format!("cannot create {}", from_path(&dir)));
  }
  rep.dir = from_path(&top);
  rep.level_dir = from_path(&dir);
  let base_id = if o.base_id != 0 {
    o.base_id
  } else {
    K_DEFAULT_BASE_ID
  };
  let mut notes = Notes::default();

  let cache = scene.cache();
  let count = scene.nodes.len().min(cache.visible.len());
  let exported_part = |i: usize| {
    if replacing {
      own_level_node(scene, i)
    } else {
      !cache.reference.get(i).copied().unwrap_or(false)
    }
  };
  // the elements of the levels next to it (replacing)
  let left_out_one = |i: usize| (replacing && cache.visible[i] && !cache.reference[i]) as usize;
  let mut left_out = 0;
  let index: HashMap<u32, usize> = (0..count).map(|i| (scene.nodes[i].id, i)).collect();
  let parent_index = |n: &Node| index.get(&n.parent).copied();

  // the levels with the game's collision: the parent of their "<level>-collision" group, and <level>
  // (a level is told from another copy of it, a reference, by that parent)
  let mut with_collision: BTreeSet<(u32, String)> = BTreeSet::new();
  for i in 0..count {
    let n = &scene.nodes[i];
    if n.kind != NodeKind::Collision || n.mesh.is_none() || !exported_part(i) {
      continue;
    }
    let Some(p) = parent_index(n) else { continue };
    let g = &scene.nodes[p];
    if let Some(level) = collision_group_level(&g.name) {
      with_collision.insert((g.parent, level.to_string()));
    }
  }
  // The pieces of a level's decor still where the game has them: its collision holds for them. The
  // game's collision of the others (deleted, moved, hidden, or given a collision of their own in
  // the editor) is taken out where they were: the level as extracted (o.originals) tells them apart.
  let mut pieces: BTreeMap<(u32, String), Pieces> = BTreeMap::new();
  if o.game_collision {
    for level in &with_collision {
      let Some(orig) = o.originals.get(&level.1) else {
        continue;
      };
      // the document's pieces of that level, shown, where they are
      let mut here: HashMap<&str, AABB> = HashMap::new();
      for i in 0..count {
        let n = &scene.nodes[i];
        if n.kind != NodeKind::Render
          || n.mesh.is_none()
          || cache.actor[i] != 0
          || !cache.visible[i]
        {
          continue;
        }
        if decor_behavior_of(&n.extras).collision.set {
          continue;
        }
        if piece_level(scene, n) == *level {
          here.insert(&n.name, cache.bounds[i]);
        }
      }
      let ps = pieces.entry(level.clone()).or_insert_with(|| Pieces {
        known: false,
        kept: HashSet::new(),
        kept_tris: TriGrid::new(4.0),
        removed_tris: TriGrid::new(4.0),
        removed: 0,
      });
      ps.known = true;
      let (mut kept, mut removed) = (Vec::new(), Vec::new());
      for n in &orig.nodes {
        if n.kind != NodeKind::Render || n.mesh.is_none() || piece_level(orig, n).1 != level.1 {
          continue;
        }
        let was = orig.world_bounds(n);
        let same = here
          .get(n.name.as_str())
          .is_some_and(|h| length(h.lo - was.lo) < 0.05 && length(h.hi - was.hi) < 0.05);
        if same {
          ps.kept.insert(n.name.clone());
          kept.push(n);
        } else {
          removed.push(n);
        }
      }
      ps.removed = removed.len();
      if removed.is_empty() {
        continue; // nothing to take out
      }
      let add = |grid: &mut TriGrid, n: &Node| {
        let w = orig.world(n);
        for prim in &n.mesh.as_ref().unwrap().prims {
          for [a, b, c] in triangles(prim) {
            grid.add(w.point(a), w.point(b), w.point(c));
          }
        }
      };
      for n in removed {
        add(&mut ps.removed_tris, n);
      }
      for n in kept {
        add(&mut ps.kept_tris, n);
      }
    }
  }
  // the game's collision holds for a piece of a level that has it, still in place
  let covered_by_game = |i: usize| {
    let n = &scene.nodes[i];
    let level = piece_level(scene, n);
    if level.1.is_empty() || !with_collision.contains(&level) {
      return false;
    }
    pieces
      .get(&level)
      .is_none_or(|p| !p.known || p.kept.contains(&n.name))
  };

  // --- the decor -------------------------------------------------------------------------------
  let mut eo = ExportOptions {
    mode: ExportMode::Builder,
    ..Default::default()
  };
  let mut hidden = 0;
  // the elements that bounce or launch Jak: actors of the level's code
  let mut decor_actions = Vec::new();
  // the elements whose collision the builder makes from their shape
  let mut by_shape = Vec::new();
  let mut bounds = AABB::default();
  for i in 0..count {
    let n = &scene.nodes[i];
    let Some(mesh) = &n.mesh else { continue };
    if n.kind != NodeKind::Render || cache.actor[i] != 0 {
      continue;
    }
    if !exported_part(i) {
      left_out += left_out_one(i);
      continue;
    }
    if !cache.visible[i] {
      hidden += 1;
      continue;
    }
    let covered = covered_by_game(i);
    eo.only.push(n.id);
    rep.meshes += 1;
    rep.triangles += mesh.tri_count();
    let b = cache.bounds.get(i).copied().unwrap_or_default();
    if b.valid() {
      bounds.add(b.lo);
      bounds.add(b.hi);
    }
    // the game's collision there, or none for see-through surfaces (water, glass): the builder
    // makes the collision of the others from their shape; a collision set in the editor (its
    // surface, an invisible wall, none) where the game's does not already hold it
    let decor = decor_behavior_of(&n.extras);
    if decor.collision.set && !(o.game_collision && covered) {
      let mut e = collision_extras(&decor.collision);
      if decor.collision.kind != "none" {
        rep.collision_from_decor += 1;
        by_shape.push(i);
        e["double_sided"] = 1.into();
      }
      eo.node_extras.insert(n.id, e);
    } else if (o.game_collision && covered) || see_through(scene, mesh) {
      eo.node_extras
        .insert(n.id, json!({"set_collision": 1, "ignore": 1}));
      if decor.collision.set {
        notes.add("export.note.decor_collision_game", &n.name, 1);
      }
    } else if n.extras.get("og_shape").is_none() && b.valid() && max3(b.size()) < K_SMALL_PIECE {
      // a small piece (a pipe, a lamp, a window, a decal): the game gives most of its details no
      // collision, and theirs would fill the cache around Jak (he falls through the floor there)
      eo.node_extras
        .insert(n.id, json!({"set_collision": 1, "ignore": 1}));
      notes.add("export.note.collision_small", &n.name, 1);
    } else {
      rep.collision_from_decor += 1;
      by_shape.push(i);
      // the decor faces either way (the game draws both sides, its ground often faces down): a
      // collision made from it holds from both sides (the game's own collision stays one-sided)
      eo.node_extras.insert(n.id, json!({"double_sided": 1}));
    }
    if decor.bounce.set || decor.launch.set {
      decor_actions.push(i);
    }
  }
  if rep.collision_from_decor > 0 {
    notes.add(
      "export.note.collision_from_decor",
      "",
      rep.collision_from_decor,
    );
  }

  // --- the game's collision: one mesh per surface, in the game's space ---------------------------
  let mut game_collision: Vec<Arc<Mesh>> = Vec::new();
  if o.game_collision {
    let mut surfaces: BTreeMap<BuilderSurface, Mesh> = BTreeMap::new();
    let mut taken_out = 0;
    for i in 0..count {
      let n = &scene.nodes[i];
      let Some(mesh) = &n.mesh else { continue };
      if n.kind != NodeKind::Collision || !exported_part(i) {
        continue;
      }
      let p = parent_index(n);
      if p.is_some_and(|p| !cache.visible[p]) {
        continue; // its level is hidden
      }
      // the pieces of its level that were taken out: their collision with them
      let level_pieces = p
        .and_then(|p| {
          let g = &scene.nodes[p];
          collision_group_level(&g.name).and_then(|l| pieces.get(&(g.parent, l.to_string())))
        })
        .filter(|ps| !ps.removed_tris.is_empty());
      // nearer to a piece taken out than to the pieces in place, by 5 cm at least, and within 8 m of
      // it: the game's collision of a piece in place lies on it, so it stays; around rubble it is
      // coarser than its pieces (the fallen palace's in the port: 1 to 5 m off them, which a closer
      // limit left standing as invisible walls across the street)
      const K_NEAR: f32 = 8.0;
      let w = &cache.worlds[i];
      for prim in &mesh.prims {
        let normals = prim.nrm.len() == prim.pos.len();
        for (t, tri) in prim.idx.as_chunks::<3>().0.iter().enumerate() {
          if tri.iter().any(|&v| v as usize >= prim.pos.len()) {
            continue;
          }
          let pat = prim.tri_pat.get(t).copied().unwrap_or(0);
          let (s, loss) = builder_surface(pat);
          let out = &mut surfaces
            .entry(s)
            .or_insert_with(|| {
              let mut m = Mesh::new();
              m.name = format!(
                "{}-collision-{:02}-{}-{}",
                o.name, s.material, s.event, s.mode
              );
              m.prims.push(Primitive::default());
              m
            })
            .prims[0];
          let v = tri.map(|k| w.point(prim.pos[k as usize]));
          let mut nrm = Vec3::new(0.0, 0.0, 0.0);
          if normals {
            // the game's normals: the builder turns the triangles that face the other way
            for &k in tri {
              nrm += w.dir(prim.nrm[k as usize]);
            }
          }
          nrm = if normals && length2(nrm) > 0.0 {
            normalize(nrm)
          } else {
            Vec3::new(0.0, 1.0, 0.0)
          };
          let before = out.idx.len();
          let mut write = |a: Vec3, b: Vec3, c: Vec3| {
            for q in [a, b, c] {
              out.idx.push(out.pos.len() as u32);
              out.pos.push(q);
              out.nrm.push(nrm);
            }
          };
          if let Some(lp) = level_pieces {
            let gone = |at: Vec3| {
              let to_removed = lp.removed_tris.distance(at, K_NEAR);
              to_removed < K_NEAR && to_removed + 0.05 < lp.kept_tris.distance(at, K_NEAR)
            };
            keep_parts(v[0], v[1], v[2], 0, &gone, &mut write);
          } else {
            write(v[0], v[1], v[2]);
          }
          let written = (out.idx.len() - before) / 3;
          if written == 0 {
            taken_out += 1;
            continue;
          }
          rep.collision_triangles += written;
          if !loss.is_empty() {
            notes.add("export.note.surfaces", &loss, 1);
          }
        }
      }
    }
    for (s, mut mesh) in surfaces {
      let p = &mut mesh.prims[0];
      p.idx.truncate(p.idx.len() / 3 * 3);
      if p.idx.is_empty() {
        continue;
      }
      if p.nrm.iter().all(|n| n.x == 0.0 && n.y == 1.0 && n.z == 0.0) {
        p.compute_normals();
      }
      let mesh = Arc::new(mesh);
      game_collision.push(mesh.clone());
      eo.extra_meshes.push((
        mesh,
        json!({
          "set_collision": 1,
          "set_invisible": 1,
          "collide_material": s.material,
          "collide_event": s.event,
          "collide_mode": s.mode,
          "noentity": s.noentity as i32,
          "nocamera": s.nocamera as i32,
          "noedge": s.noedge as i32,
          "nolineofsight": s.nolineofsight as i32,
        }),
      ));
    }
    let pieces_out: usize = pieces.values().map(|ps| ps.removed).sum();
    if taken_out > 0 {
      notes.add(
        "export.note.collision_taken_out",
        &pieces_out.to_string(),
        1,
      );
      if taken_out > 1 {
        notes.add("export.note.collision_taken_out", "", taken_out - 1);
      }
    }
  }

  // --- where Jak would meet more collision than the game gathers at once -----------------------
  {
    let mut all = TriGrid::new(2.0);
    for mesh in &game_collision {
      for prim in &mesh.prims {
        for [a, b, c] in triangles(prim) {
          all.add(a, b, c);
        }
      }
    }
    // the faces made from the decor's shape twice: the builder doubles them
    for &i in &by_shape {
      let w = &cache.worlds[i];
      for prim in &scene.nodes[i].mesh.as_ref().unwrap().prims {
        for [a, b, c] in triangles(prim) {
          for _side in 0..2 {
            all.add(w.point(a), w.point(b), w.point(c));
          }
        }
      }
    }
    let dense = collision_hot_spots(&all, 1);
    for (n, at) in dense.iter().take(3) {
      notes.add(
        "export.note.collision_dense",
        &format!("({:.0}, {:.0}, {:.0}): {n}", at.x, at.y, at.z),
        1,
      );
    }
    if dense.len() > 3 {
      notes.add("export.note.collision_dense", "", dense.len() - 3);
    }
    rep.dense_places = dense.len();
    rep.densest = dense.into_iter().take(100).collect();
  }

  // --- the actors ------------------------------------------------------------------------------
  // first the actors exported and their names (no other actor of the level has it), by their id
  // in the game: the links between actors by id become links by name (the builder numbers the
  // actors again)
  let mut exported: Vec<Exported> = Vec::new();
  let mut name_of_aid: HashMap<u32, String> = HashMap::new();
  // the name in the editor -> in the level
  let mut name_of_node: HashMap<String, String> = HashMap::new();
  let mut absent = 0;
  {
    let mut names = HashSet::new();
    for i in 0..count {
      let n = &scene.nodes[i];
      if n.kind != NodeKind::Actor {
        continue;
      }
      let etype = n
        .extras
        .get("etype")
        .and_then(Value::as_str)
        .unwrap_or_default();
      // without actors: decor alone, but the platforms and bridges needed to get around stay
      // (the same rule the document itself loads a level by)
      if !o.actors && !data.is_some_and(|d| d.object_family(etype) == "platform") {
        continue;
      }
      if !exported_part(i) {
        left_out += left_out_one(i);
        continue;
      }
      if !cache.visible[i] {
        hidden += 1;
        continue;
      }
      if etype.is_empty() {
        continue;
      }
      if data.is_some_and(|d| !d.has_type(etype)) {
        notes.add("export.note.types_missing", etype, 1);
        continue;
      }
      // never there (its behavior): not in the level
      let present = n
        .extras
        .get("og_behavior")
        .and_then(|b| b.get("present"))
        .unwrap_or(&Value::Null);
      if condition_of(present, Condition::default()).kind == ConditionKind::Never {
        absent += 1;
        continue;
      }
      let base = if n.name.is_empty() { etype } else { &n.name };
      let mut name = base.to_string();
      let mut k = 2;
      while names.contains(&name) {
        name = format!("{base}-{k}");
        k += 1;
      }
      names.insert(name.clone());
      // a battle: its creatures come from a table of the game's code (by its extra-id), their
      // models with the levels that have them; spawned in a level of its own, the first one kills
      // the game (left out: "A creature appears here" places them)
      if etype == "battle" {
        notes.add("export.note.battle", &n.name, 1);
        continue;
      }
      // an object whose code gets a level of the game by its name (its own): in a level of another
      // name, that one not loaded, the game dies of it (left out)
      if let Some(level) = data.and_then(|d| {
        d.needs_levels(etype)
          .iter()
          .find(|l| **l != o.name && **l != o.replaces)
      }) {
        notes.add(
          "export.note.needs_level",
          &format!("{} ({level})", n.name),
          1,
        );
        continue;
      }
      if !n.name.is_empty() {
        name_of_node
          .entry(n.name.clone())
          .or_insert_with(|| name.clone());
      }
      if let Some(aid) = n.extras.get("aid").filter(|a| is_integer(a)) {
        let aid = aid
          .as_i64()
          .map_or_else(|| aid.as_u64().unwrap_or(0) as u32, |a| a as u32);
        name_of_aid.insert(aid, name.clone());
      }
      exported.push(Exported {
        index: i,
        ty: etype.to_string(),
        name,
      });
    }
  }
  let mut actor_lines: Vec<String> = Vec::new();
  let mut art_groups: BTreeSet<String> = BTreeSet::new();
  let mut kill_masks = 0;
  // the code of the level (<name>-obs.gc)
  let mut generated = String::new();
  // the code of the game the level needs, in order
  let mut code_files: Vec<String> = Vec::new();
  // the lists it is made of, each in its DGO's order (ordered together at the end)
  let mut code_lists: Vec<Vec<String>> = Vec::new();
  // The code the game already has around the level replaced: the levels it shows with it (its
  // neighbors, the levels of its continue points; an alias of a level, as ctywide-kg, is that
  // level, ctywide). The DGO does not bring it again (the effects of the city draw particle
  // groups of ctywide-part, which needs the code of the whole city).
  let mut loaded_around: HashSet<String> = HashSet::new();
  if let (Some(replaced), Some(data)) = (replaced, data) {
    let mut around: BTreeSet<String> = replaced.neighbors.iter().cloned().collect();
    for c in &replaced.continues {
      around.extend(c.shows.iter().cloned());
    }
    around.remove(&o.replaces);
    for mut name in around {
      let mut l = data.level(&name);
      while l.is_none_or(|l| l.code.is_empty())
        && let Some(dash) = name.rfind('-')
      {
        name.truncate(dash);
        l = data.level(&name);
      }
      if let Some(l) = l
        && name != o.replaces
      {
        loaded_around.extend(l.code.iter().cloned());
      }
    }
  }
  let mut need = |files: &[String]| {
    code_lists.push(files.to_vec());
    for f in files {
      if !loaded_around.contains(f) && !code_files.contains(f) {
        code_files.push(f.clone());
      }
    }
  };
  // the level replaced: its own code (its particles, its objects, what its missions spawn there)
  if let Some(r) = replaced {
    need(&r.code);
  }
  for e in &exported {
    let n = &scene.nodes[e.index];
    // the code of its type, and of the particle group it draws (a part-spawner's art-name)
    if let Some(data) = data {
      need(&data.code_files(&e.ty));
      if let Some(art) = n
        .extras
        .get("lump")
        .and_then(|l| l.get("art-name"))
        .and_then(Value::as_str)
      {
        need(&data.effect_code_files(strip_quote(art)));
      }
    }
    // its behavior, as a type of the level deriving its own, the actors it names by their names in
    // the level
    let mut etype = e.ty.clone();
    if let Value::Object(mut behavior) = n
      .extras
      .get("og_behavior")
      .cloned()
      .unwrap_or_else(|| json!({}))
    {
      for c in behavior.values_mut() {
        let renamed = c
          .get("arg")
          .and_then(Value::as_str)
          .and_then(|a| name_of_node.get(a))
          .cloned();
        if let Some(name) = renamed {
          c["arg"] = name.into();
        }
      }
      let own = behavior_type_name(&o.name, &e.name);
      let code = behavior_code(
        data,
        &e.ty,
        &Value::Object(behavior),
        &own,
        &format!("{} ({})", e.name, e.ty),
      );
      if !code.is_empty() {
        generated += &code;
        generated.push('\n');
        etype = own;
        rep.behaviors += 1;
      }
    }
    // where it was in the game: the positions inside its data move with it
    let mut moved = Mat4::identity();
    let mut has_origin = false;
    if let Some(og) = n
      .extras
      .get("og_origin")
      .and_then(Value::as_array)
      .filter(|a| a.len() == 7)
    {
      let f = |k: usize| og[k].as_f64().unwrap_or(0.0) as f32;
      let t0 = Transform {
        t: Vec3::new(f(0), f(1), f(2)),
        r: Quat::new(f(3), f(4), f(5), f(6)),
        ..Default::default()
      };
      let mut t1 = Transform::from_matrix(&cache.worlds[e.index]);
      t1.s = Vec3::new(1.0, 1.0, 1.0);
      moved = t1.matrix() * t0.matrix().inverse();
      has_origin = true;
    }
    let path_tags: HashSet<&str> = data
      .and_then(|d| d.params(&e.ty))
      .map(|ps| {
        ps.iter()
          .filter(|p| p.kind == "path")
          .map(|p| p.tag.as_str())
          .collect()
      })
      .unwrap_or_default();

    let w = &cache.worlds[e.index];
    let pos = w.point(Vec3::new(0.0, 0.0, 0.0));
    let q = Transform::from_matrix(w).r.normalized();
    let mut radius = n
      .extras
      .get("bsphere")
      .and_then(Value::as_array)
      .filter(|b| b.len() == 4)
      .and_then(|b| b[3].as_f64())
      .unwrap_or(0.0) as f32;
    if radius <= 0.0 {
      // a placed one: its model's
      for c in scene.children(n.id) {
        if let Some(mesh) = scene.find(c).and_then(|m| m.mesh.as_ref()) {
          radius = radius.max(mesh.bounds().radius());
        }
      }
      if radius <= 0.0 {
        radius = 5.0;
      }
    }

    // its data (res lumps)
    let mut lump = Map::new();
    lump.insert("name".into(), e.name.clone().into());
    let types = n.extras.get("lump_types").filter(|t| t.is_object());
    if let Some(Value::Object(lj)) = n.extras.get("lump") {
      for (tag, v) in lj {
        if tag == "name" || tag == "visvol" {
          continue; // its name is set, its visibility box is the game's
        }
        let ltype = types
          .and_then(|t| t.get(tag))
          .and_then(Value::as_str)
          .unwrap_or_default();
        if tag == "kill-mask" {
          // when the actor is there along the story of its level: the new level has its own
          if v.as_f64() != Some(0.0) {
            kill_masks += 1;
          }
          continue;
        }
        if ltype == "actor-name" {
          // a link set in the editor: another actor by its name in the editor, then in the level
          match v.as_str().and_then(|s| name_of_node.get(s)) {
            Some(name) => {
              lump.insert(tag.clone(), json!(["string", name]));
            }
            None => notes.add("export.note.links_dropped", tag, 1),
          }
          continue;
        }
        if (ltype == "uint32" || ltype == "int32" || ltype.is_empty())
          && tag.len() > 6
          && tag.ends_with("-actor")
          && tag != "task-actor"
        {
          // other actors by their ids: by their names, when they are in the level
          let mut ids = Vec::new();
          numbers(v, &mut ids);
          let mut linked = vec![Value::from("string")];
          for id in &ids {
            let aid = id.as_f64().unwrap_or(0.0) as i64 as u32;
            let Some(name) = name_of_aid.get(&aid) else {
              break;
            };
            linked.push(name.clone().into());
          }
          if ids.is_empty() || linked.len() != ids.len() + 1 {
            notes.add("export.note.links_dropped", tag, 1);
            continue;
          }
          lump.insert(tag.clone(), Value::Array(linked));
          continue;
        }
        if ltype == "pair" {
          // a script of the game: the level builder cannot write them (the behavior set in the
          // editor is the level's code instead)
          notes.add("export.note.scripts_dropped", tag, 1);
          continue;
        }
        if matches!(ltype, "int8" | "int16" | "uint8" | "uint16") {
          notes.add("export.note.lumps_widened", tag, 1);
        }
        let mut bv = builder_lump_value(v, ltype);
        if has_origin
          && path_tags.contains(tag.as_str())
          && let Some(points) = bv.as_array_mut()
          && points.len() > 1
          && points[0] == "vector"
        {
          // a path, in game units: moved as the actor was
          for pt in &mut points[1..] {
            let c = |k: usize| pt[k].as_f64().unwrap_or(0.0) as f32;
            let p = moved.point(Vec3::new(c(0), c(1), c(2)) / 4096.0) * 4096.0;
            pt[0] = p.x.into();
            pt[1] = p.y.into();
            pt[2] = p.z.into();
          }
        }
        if bv.is_null() {
          let what = if ltype.is_empty() {
            tag.clone()
          } else {
            format!("{tag} ({ltype})")
          };
          notes.add("export.note.lumps_dropped", &what, 1);
          continue;
        }
        lump.insert(tag.clone(), bv);
      }
    }
    let game_task = n
      .extras
      .get("game_task")
      .and_then(Value::as_i64)
      .unwrap_or(0) as i32;
    actor_lines.push(format!(
      "{{\"trans\": [{}, {}, {}], \"etype\": \"{etype}\", \"game_task\": {game_task}, \"quat\": [{}, {}, {}, {}], \"bsphere\": [{}, {}, {}, {}], \"lump\": {}}}",
      num(pos.x as f64),
      num(pos.y as f64),
      num(pos.z as f64),
      num(q.x as f64),
      num(q.y as f64),
      num(q.z as f64),
      num(q.w as f64),
      num(pos.x as f64),
      num(pos.y as f64),
      num(pos.z as f64),
      num(radius as f64),
      Value::Object(lump)
    ));
    rep.actors += 1;
    // the models its type draws: their art groups, from the game's DGOs
    if let Some(data) = data
      && let Some(models) = data.type_models(&e.ty)
    {
      for m in models {
        let ag = data.level_art_group(m);
        if !ag.is_empty() {
          art_groups.insert(format!("{ag}-ag"));
        }
      }
    }
  }
  // the decor that bounces or launches Jak: an actor of the level's code on its top
  {
    let mut taken: HashSet<String> = exported.iter().map(|e| e.name.clone()).collect();
    for &i in &decor_actions {
      let n = &scene.nodes[i];
      let decor = decor_behavior_of(&n.extras);
      let b = cache.bounds[i];
      if !b.valid() {
        continue;
      }
      let top = Vec3::new(b.center().x, b.hi.y, b.center().z);
      let radius = (b.size().x.max(b.size().z) * 0.5).clamp(1.0, 20.0);
      let mut add_actor = |what: &str, code_of_type: String, mut lump: Value, ty: &str| {
        let base = format!("{}-{what}", n.name);
        let mut name = base.clone();
        let mut k = 2;
        while taken.contains(&name) {
          name = format!("{base}-{k}");
          k += 1;
        }
        taken.insert(name.clone());
        generated += &code_of_type;
        generated.push('\n');
        rep.behaviors += 1;
        lump["name"] = name.into();
        actor_lines.push(format!(
          "{{\"trans\": [{}, {}, {}], \"etype\": \"{ty}\", \"game_task\": 0, \"quat\": [0.0, 0.0, 0.0, 1.0], \"bsphere\": [{}, {}, {}, {}], \"lump\": {lump}}}",
          num(top.x as f64),
          num(top.y as f64),
          num(top.z as f64),
          num(top.x as f64),
          num(top.y as f64),
          num(top.z as f64),
          num(radius as f64)
        ));
        rep.actors += 1;
      };
      if decor.bounce.set {
        let ty = behavior_type_name(&o.name, &format!("{}-bounce", n.name));
        let comment = format!(
          "{}: throws Jak up {} m",
          n.name,
          num(decor.bounce.height as f64)
        );
        add_actor(
          "bounce",
          bounce_code(&ty, radius, &comment),
          json!({"spring-height": ["float", decor.bounce.height * 4096.0]}),
          &ty,
        );
      }
      if decor.launch.set {
        // where Jak lands: `distance` along the element's facing turned by `yaw`, `height` above
        let t = Transform::from_matrix(&cache.worlds[i]);
        let mut facing = t.r.rotate(Vec3::new(0.0, 0.0, 1.0));
        facing.y = 0.0;
        facing = if length2(facing) > 1e-6 {
          normalize(facing)
        } else {
          Vec3::new(0.0, 0.0, 1.0)
        };
        let dir =
          Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(decor.launch.yaw)).rotate(facing);
        let to = top + dir * decor.launch.distance + Vec3::new(0.0, decor.launch.height, 0.0);
        let to_units = [to.x * 4096.0, to.y * 4096.0, to.z * 4096.0];
        let ty = behavior_type_name(&o.name, &format!("{}-launch", n.name));
        let comment = format!(
          "{}: throws Jak {} m away, {} m up",
          n.name,
          num(decor.launch.distance as f64),
          num(decor.launch.height as f64)
        );
        add_actor(
          "launch",
          launch_code(&ty, radius, to_units, decor.launch.time, &comment),
          json!({}),
          &ty,
        );
      }
    }
  }
  if kill_masks > 0 {
    notes.add("export.note.kill_masks", "", kill_masks);
  }
  if left_out > 0 {
    notes.add("export.note.replace_left_out", "", left_out);
  }
  // where the game starts the player in the level replaced: on the ground of the new one?
  if replaced.is_some() {
    for c in check_continues(scene, data, &o.replaces) {
      if !c.ok() {
        notes.add("export.note.continue_no_ground", &c.name, 1);
      }
    }
  }
  if absent > 0 {
    notes.add("export.note.absent", "", absent);
  }
  if hidden > 0 {
    notes.add("export.note.hidden", "", hidden);
  }
  rep.art_groups = art_groups.len();

  // --- the files -------------------------------------------------------------------------------
  let glb = dir.join(format!("{}.glb", o.name));
  export_glb(scene, &glb, &eo)?;
  rep.files.push(from_path(&glb));

  let mut nav_line = String::new();
  if !o.navmeshes.is_empty() {
    // their ids after the actors', far from them
    let first = base_id + rep.actors as u32 + 1000;
    let entries: Vec<NavExportEntry> = o
      .navmeshes
      .iter()
      .enumerate()
      .map(|(i, mesh)| NavExportEntry {
        mesh,
        aid: first + i as u32,
        name: o
          .navmesh_names
          .get(i)
          .cloned()
          .unwrap_or_else(|| format!("nav-mesh-{i}")),
      })
      .collect();
    let nav = dir.join(format!("{}-nav.json", o.name));
    write_navmesh_blob(&entries, &nav)?;
    rep.files.push(from_path(&nav));
    rep.navmeshes = entries.len();
    nav_line = format!("  \"nav_data\": \"{install_dir}{}-nav.json\",\n", o.name);
  }

  // the regions of the level replaced: the levels the game loads and shows around it
  let mut regions_line = String::new();
  if replacing && o.region_trees.as_object().is_some_and(|t| !t.is_empty()) {
    let regions = builder_regions(&o.region_trees, base_id + 2000, &mut rep.regions);
    regions_line = format!(
      "  // where the game loads and shows the levels around this one (the level replaced's)\n  \"region_trees\": {regions},\n"
    );
  } else if replacing && !o.region_trees.is_object() {
    // unknown (extracted before the regions were): {} is a level that has none (ctywide)
    notes.add("export.note.regions_missing", &o.replaces, 1);
  }

  // the level for the builder
  let from = if scene.settings.level.is_empty() {
    "a new level"
  } else {
    &scene.settings.level
  };
  let auto_walls = rep.collision_from_decor > 0 || !o.game_collision;
  let art_list: Vec<String> = art_groups.iter().map(|ag| format!("\"{ag}\"")).collect();
  let mut j = format!(
    "// {name}: exported by Jak Level Editor for Jak 3 from {from}.
// To add it to the game: README.md, at the top of the export's folder.
{{
  \"long_name\": \"{name}\",
  \"iso_name\": \"{iso}\",
  \"nickname\": \"{nick}\",
  \"gltf_file\": \"{install_dir}{name}.glb\",
  // the game's collision is in the file; the builder makes the rest from the decor's shape
  \"automatic_wall_detection\": {auto_walls},
  \"automatic_wall_angle\": 45.0,
  \"double_sided_collide\": {double},
  \"base_id\": {base_id},
  \"base_region_id\": {base_id},
  \"cameras\": [],
{regions_line}{nav_line}  \"custom_models\": [],
  \"textures\": [],
  \"art_groups\": [{arts}],
  \"actors\": [
",
    name = o.name,
    iso = o.iso_name,
    nick = o.nickname,
    // the faces made from the decor are doubled each (extras double_sided, the builder of the
    // editor's fork); all of them when the level has no collision of the game's
    double = rep.collision_from_decor > 0 && !o.game_collision,
    arts = art_list.join(", "),
  );
  for (i, line) in actor_lines.iter().enumerate() {
    j += &format!(
      "    {line}{}",
      if i + 1 < actor_lines.len() {
        ",\n"
      } else {
        "\n"
      }
    );
  }
  j += "  ]\n}\n";
  let jsonc = dir.join(format!("{}.jsonc", o.name));
  write_text(&jsonc, &j)?;
  rep.files.push(from_path(&jsonc));

  // the level's code: the behaviors set in the editor
  let code_stem = format!("{}-obs", o.name);
  if !generated.is_empty() {
    let gc = format!(
      ";;-*-Lisp-*-
(in-package goal)

;; {}: the behaviors of its objects, set in Jak Level Editor. Each object
;; whose behavior differs from the game's has a type of its own deriving the game's.

{generated}",
      o.name
    );
    let gc_dir = top
      .join("goal_src")
      .join("jak3")
      .join("levels")
      .join(&o.name);
    let _ = std::fs::create_dir_all(&gc_dir);
    let gc_path = gc_dir.join(format!("{code_stem}.gc"));
    write_text(&gc_path, &gc)?;
    rep.files.push(from_path(&gc_path));
    rep.code_file = from_path(&gc_path);
  }

  // the DGO
  // in an order the game can load: the parents of the types first, from whichever DGO
  if let Some(data) = data {
    code_files = data.order_code(&code_files, &code_lists, &loaded_around);
  }
  let nick_upper = o.nickname.to_ascii_uppercase();
  let mut gd = format!(
    ";; DGO definition file for {}, exported by Jak Level Editor for Jak 3.\n(\"{nick_upper}.DGO\"\n (\n",
    o.name
  );
  // the code of the game its objects need beyond the common code, then its own
  for f in &code_files {
    gd += &format!("  \"{f}.o\"\n");
  }
  if !generated.is_empty() {
    gd += &format!("  \"{code_stem}.o\"\n");
  }
  for ag in &art_groups {
    gd += &format!("  \"{ag}.go\"\n");
  }
  gd += &format!("  \"{}.go\"\n  ))\n", o.name);
  let gd_path = dir.join(format!("{}.gd", o.name));
  write_text(&gd_path, &gd)?;
  rep.files.push(from_path(&gd_path));

  // the level's definition in the game's code
  let bottom = if bounds.valid() {
    bounds.lo.y
  } else {
    o.start.y
  } - 20.0;
  let info_path = dir.join("level-info.gc");
  let info_text = if let Some(r) = replaced {
    // the game's own definition stays (continue points, music, missions...): its nickname names the
    // DGO the game loads for it
    format!(
      ";; {name} replaces {rep_name} in the game. In goal_src/jak3/engine/level/level-info.gc,
;; in (define {rep_name} ...), the game loads <nickname>.DGO for the level: change its
;; nickname so that it loads {nick_upper}.DGO, this level (put '{old} back to play the game's own {rep_name}):
;;
;;   :nickname '{old}
;; becomes
    :nickname '{nick}
",
      name = o.name,
      rep_name = o.replaces,
      old = r.nickname,
      nick = o.nickname
    )
  } else {
    format!(
      ";; add this to goal_src/jak3/engine/level/level-info.gc\n\n{}",
      level_info(o, bottom)
    )
  };
  write_text(&info_path, &info_text)?;
  rep.files.push(from_path(&info_path));

  rep.notes = notes.list();

  // the lines of game.gp: the level, its code (after the common code and the code of the game it
  // derives from), its DGO
  let mut game_gp = vec![format!("(build-custom-level \"{}\")", o.name)];
  if !generated.is_empty() {
    let mut line = format!(
      "(goal-src \"levels/{}/{code_stem}.gc\" \"entity-debug\"",
      o.name
    );
    for f in &code_files {
      line += &format!(" \"{f}\"");
    }
    game_gp.push(line + ")");
  }
  game_gp.push(format!(
    "(custom-level-cgo \"{nick_upper}.DGO\" \"{0}/{0}.gd\")",
    o.name
  ));

  // how to add it to the game
  let readme = readme(
    scene,
    o,
    &rep,
    replaced,
    &install_dir,
    &code_dir,
    &code_stem,
    &generated,
    &game_gp,
  );
  let md_path = top.join("README.md");
  write_text(&md_path, &readme)?;

  rep.files.push(from_path(&md_path));
  Ok(rep)
}

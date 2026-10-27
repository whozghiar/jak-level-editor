//! The MCP server's tools that find things: elements by name or kind, an element, a ray,
//! assemblies.

use super::*;

impl Editor {
  pub(super) fn tool_find_nodes(&mut self, a: &Value) -> R {
    let name = lower(str_arg(a, "name"));
    let etype = str_arg(a, "etype");
    let kind = str_arg(a, "kind");
    let include_locked = flag(a, "include_locked", false);
    let near = vec3_arg(a, "near");
    let radius = num(a, "radius", 20.0);
    let limit = int(a, "limit", 50).clamp(1, 20000) as usize;
    // inside a box (its middle), or outside it; under a group (a level of the document)
    let bx = a.get("box").and_then(box_arg);
    let outside = flag(a, "outside", false);
    let under = uint(a, "under");
    let cache = self.scene.cache();
    let mut found: Vec<(f32, u32)> = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate() {
      // an actor's model: the actor stands for it
      if n.extras.get("og_actor_visual").is_some() || n.kind == NodeKind::Collision {
        continue;
      }
      if (n.locked && !include_locked) || (!kind.is_empty() && kind != kind_name(n.kind)) {
        continue;
      }
      if !name.is_empty() && !lower(&n.name).contains(&name) {
        continue;
      }
      if !etype.is_empty() && (n.kind != NodeKind::Actor || str_arg(&n.extras, "etype") != etype) {
        continue;
      }
      if n.kind == NodeKind::Group && name.is_empty() && etype.is_empty() && kind.is_empty() {
        continue;
      }
      if under != 0 {
        let mut p = self.scene.find(n.parent);
        let mut guard = 0;
        while let Some(x) = p.filter(|x| x.id != under && guard < 1000) {
          p = self.scene.find(x.parent);
          guard += 1;
        }
        if p.is_none_or(|x| x.id != under) {
          continue;
        }
      }
      let in_cache = i < cache.count;
      let nb = if n.mesh.is_some() && in_cache {
        cache.bounds[i]
      } else {
        AABB::default()
      };
      let mid = if nb.valid() {
        nb.center()
      } else if in_cache {
        cache.worlds[i].col3(3)
      } else {
        self.scene.world(n).col3(3)
      };
      if let Some(b) = &bx {
        let inside = mid.x >= b.lo.x
          && mid.x <= b.hi.x
          && mid.y >= b.lo.y
          && mid.y <= b.hi.y
          && mid.z >= b.lo.z
          && mid.z <= b.hi.z;
        if inside == outside {
          continue;
        }
      }
      let mut d = 0.0;
      if let Some(p) = near {
        d = length(mid - p);
        if d > radius {
          continue;
        }
      }
      found.push((d, n.id));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let list: Vec<Value> = found
      .iter()
      .take(limit)
      .map(|&(_, id)| self.node_json(id))
      .collect();
    Ok(json!({"total": found.len(), "nodes": list}))
  }

  pub(super) fn tool_get_node(&mut self, a: &Value) -> R {
    let id = uint(a, "id");
    let n = self
      .scene
      .find(id)
      .cloned()
      .ok_or("no element with that id")?;
    let mut j = self.node_json(id);
    // what the inspector's Preview shows of it (an actor, a model of the catalog)
    if let Some(p) = self.preview_summary(id) {
      j["preview"] = p;
    }
    j["parent"] = json!(n.parent);
    j["children"] = json!(self.scene.children(id));
    j["local"] = json!({
      "position": v3(n.local.t),
      "rotation_deg": v3(n.local.r.to_euler_deg()),
      "scale": v3(n.local.s),
    });
    if let Some(mesh) = &n.mesh {
      j["mesh"] = json!({"name": mesh.name, "triangles": mesh.tri_count()});
      // its materials (set_material), their textures
      let mut seen = BTreeSet::new();
      let mut mats = Vec::new();
      for p in &mesh.prims {
        let Some(m) = usize::try_from(p.material)
          .ok()
          .and_then(|i| self.scene.materials.get(i))
        else {
          continue;
        };
        if !seen.insert(p.material) {
          continue;
        }
        let c = m.base_color;
        let mut e = json!({"material": p.material, "color": [c.x, c.y, c.z, c.w]});
        if let Some(t) = usize::try_from(m.texture)
          .ok()
          .and_then(|i| self.scene.textures.get(i))
        {
          e["texture"] = json!(t.name);
        }
        mats.push(e);
      }
      j["materials"] = json!(mats);
    }
    if let Some(shape) = node_shape(&n) {
      j["shape"] = shape.to_json();
    }
    // a structure of Jak II: where it comes from and what was checked when it came in
    if let Some(im) = n.extras.get("og_import") {
      j["import"] = im.clone();
    }
    // what the game does with it (collision, bounce, launch)
    if let Some(d) = n.extras.get("og_decor") {
      j["decor"] = d.clone();
    }
    if n.kind == NodeKind::Actor {
      j["game_data"] = n.extras.clone();
      // what configures it: the lumps its type's code reads (set_actor), and its behavior
      let data = self.game_data();
      let etype = str_arg(&n.extras, "etype");
      if let Some(ps) = data.as_ref().and_then(|d| d.params(etype)) {
        j["params"] = ps
          .iter()
          .map(|p| json!({"tag": p.tag, "kind": p.kind, "default": p.def}))
          .collect();
      }
      j["behavior_kind"] =
        json!(format!("{:?}", behavior_kind(data.as_deref(), etype)).to_lowercase());
      for c in self.scene.children(id) {
        if let Some(cn) = self
          .scene
          .find(c)
          .filter(|c| c.extras.get("og_actor_visual").is_some())
        {
          j["model"] = json!(cn.name);
        }
      }
    }
    Ok(j)
  }

  pub(super) fn tool_raycast(&mut self, a: &Value) -> R {
    let origin = vec3_arg(a, "origin").ok_or("origin: [x, y, z]")?;
    let direction = vec3_arg(a, "direction").unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    if length(direction) < 1e-6 {
      return Err("direction: a vector".into());
    }
    let f = PickFilter {
      actors: flag(a, "actors", false),
      include_locked: true,
      ..Default::default()
    };
    let hit = pick(&self.scene, &Ray::new(origin, normalize(direction)), &f);
    if !hit.hit() {
      return Ok(json!({"hit": false}));
    }
    let mut j = json!({
      "hit": true,
      "point": v3(hit.point),
      "normal": v3(hit.normal),
      "distance": round3(hit.t),
    });
    if self.scene.find(hit.node).is_some() {
      j["node"] = self.node_json(hit.node);
    }
    Ok(j)
  }

  pub(super) fn tool_list_assemblies(&mut self, a: &Value) -> R {
    let min_elements = int(a, "min_elements", 2).max(1) as usize;
    let min_size = num(a, "min_size", 4.0);
    let max_size = num(a, "max_size", 60.0).max(10.0);
    let limit = int(a, "limit", 200).max(1) as usize;
    let assemblies = find_assemblies(&mut self.scene, max_size);
    let c = self.scene.cache();
    // seed: the largest element whose name no other has (a recipe finds the assembly by it)
    let mut named: HashMap<&str, usize> = HashMap::new();
    for n in &self.scene.nodes {
      *named.entry(n.name.as_str()).or_default() += 1;
    }
    let mut list = Vec::new();
    let mut total = 0;
    for ids in &assemblies {
      if ids.len() < min_elements {
        continue;
      }
      let mut bx = AABB::default();
      let (mut main, mut seed) = ("", "");
      let (mut main_volume, mut seed_volume) = (-1.0f32, -1.0f32);
      // a part's name without its number: how many, their volume
      let mut parts: BTreeMap<&str, (i32, f32)> = BTreeMap::new();
      let mut triangles = 0;
      // the largest element's size (meters)
      let mut largest = 0.0f32;
      for &id in ids {
        let Some(i) = self.scene.index_of(id).filter(|&i| i < c.count) else {
          continue;
        };
        let b = c.bounds[i];
        bx.add_box(&b);
        let s = b.size();
        let volume = s.x * s.y * s.z;
        let name = self.scene.nodes[i].name.as_str();
        if volume > main_volume {
          main_volume = volume;
          main = name;
        }
        if named[name] == 1 && volume > seed_volume {
          seed_volume = volume;
          seed = name;
        }
        let stem = match name.rfind('-') {
          Some(dash)
            if dash + 1 < name.len() && name[dash + 1..].bytes().all(|b| b.is_ascii_digit()) =>
          {
            &name[..dash]
          }
          _ => name,
        };
        largest = largest.max(s.x).max(s.y).max(s.z);
        let p = parts.entry(stem).or_default();
        p.0 += 1;
        p.1 += volume;
        if let Some(m) = &self.scene.nodes[i].mesh {
          triangles += m.tri_count();
        }
      }
      let size = bx.size();
      if !bx.valid() || size.x.max(size.y).max(size.z) < min_size {
        continue;
      }
      total += 1;
      if list.len() >= limit {
        continue;
      }
      let mut by_count: Vec<(i32, &str)> = parts.iter().map(|(&s, n)| (-n.0, s)).collect();
      by_count.sort();
      let names: Vec<Value> = by_count
        .iter()
        .take(6)
        .map(|&(n, s)| json!([s, -n, parts[s].1.round()]))
        .collect();
      list.push(json!({
        "ids": ids,
        "elements": ids.len(),
        "triangles": triangles,
        "largest": largest,
        "box": [[bx.lo.x, bx.lo.y, bx.lo.z], [bx.hi.x, bx.hi.y, bx.hi.z]],
        "main": main,
        "seed": seed,
        "parts": names,
      }));
    }
    Ok(json!({"assemblies": list, "count": total}))
  }

  /// For the studies of the level pieces (not offered to the assistant): the large pieces of the
  /// levels' shell (og_terrain), the ground locked or not, what stands more than 3 m above their
  /// floor (flat: roofs, upright: walls), the walls split off
  pub(super) fn ground_report(&mut self) -> Value {
    struct Tri {
      area: f32,
      ny: f32,
      top: f32,
      mid: f32,
    }
    let cache = self.scene.cache();
    let mut pieces = Vec::new();
    for (i, n) in self.scene.nodes.iter().enumerate().take(cache.count) {
      let Some(mesh) = n
        .mesh
        .as_ref()
        .filter(|_| n.kind == NodeKind::Render && n.extras.get("og_terrain").is_some())
      else {
        continue;
      };
      let w = cache.worlds[i];
      let mut flats: Vec<(f32, f32)> = Vec::new(); // height, area
      let (mut all, mut flat, mut walk) = (0.0f64, 0.0f64, 0.0f64);
      let mut tris = Vec::new();
      let mut b = AABB::default();
      for pr in &mesh.prims {
        for t in pr.idx.chunks_exact(3) {
          let [p, q, r] = [0, 1, 2].map(|k| w.point(pr.pos[t[k] as usize]));
          b.add(p);
          b.add(q);
          b.add(r);
          let nr = crate::math::cross(q - p, r - p);
          let l = length(nr);
          if l < 1e-8 {
            continue;
          }
          let tri = Tri {
            area: l * 0.5,
            ny: nr.y.abs() / l,
            top: p.y.max(q.y).max(r.y),
            mid: (p.y + q.y + r.y) / 3.0,
          };
          all += tri.area as f64;
          if tri.ny > 0.7 {
            flat += tri.area as f64;
            flats.push((tri.mid, tri.area));
          }
          if tri.ny > 0.5 {
            walk += tri.area as f64;
          }
          tris.push(tri);
        }
      }
      if all <= 0.0 {
        continue;
      }
      flats.sort_by(|a, b| a.0.total_cmp(&b.0));
      let mut base = flats.first().map_or(b.lo.y, |f| f.0);
      let mut acc = 0.0f64;
      for &(y, ar) in &flats {
        base = y;
        acc += ar as f64;
        if acc >= 0.2 * flat {
          break;
        }
      }
      let (mut above_flat, mut above_up) = (0.0f64, 0.0f64);
      let mut high = base;
      for t in &tris {
        let is_flat = t.ny >= 0.7;
        if !(if is_flat {
          t.mid > base + 3.0
        } else {
          t.top > base + 3.0
        }) {
          continue;
        }
        if is_flat {
          above_flat += t.area as f64;
        } else if t.ny < 0.3 {
          above_up += t.area as f64;
        }
        high = high.max(t.top);
      }
      pieces.push(json!({
        "id": n.id,
        "name": n.name,
        "locked": n.locked,
        "split": n.extras.get("og_split").is_some(),
        "walls_piece": n.name.len() > 6 && n.name.ends_with("-walls"),
        "box": [v3(b.lo), v3(b.hi)],
        "area": all,
        "flat": flat / all,
        "walkable": walk / all,
        "base": base,
        "above_flat": above_flat,
        "above_upright": above_up,
        "high": high - base,
        "covered1": covered_below(&self.scene, i, 1.0),
        "covered3": covered_below(&self.scene, i, 3.0),
      }));
    }
    json!({"level": self.scene.settings.level, "pieces": pieces})
  }
}

//! The level's collision: the game's where a level's decor is still in place, the default of
//! each element (made from its shape, or none), the spots the builder's cache cannot hold.

use super::*;

/// a surface nothing stands on: glows and light shafts, water and glass drawn translucent (the
/// levels' own materials are blended with their texture's alpha almost everywhere: that alone does
/// not make them see-through), the same rule as the nav meshes'
pub(super) fn see_through(scene: &Scene, mesh: &Mesh) -> bool {
  let mut any = false;
  for p in &mesh.prims {
    if p.idx.is_empty() {
      continue;
    }
    match usize::try_from(p.material)
      .ok()
      .and_then(|m| scene.materials.get(m))
    {
      Some(m) if navmesh_ignores(m) => any = true,
      _ => return false,
    }
  }
  any
}

/// the triangles of a primitive (those whose corners it has)
pub(super) fn triangles(p: &Primitive) -> impl Iterator<Item = [Vec3; 3]> + '_ {
  p.idx.as_chunks::<3>().0.iter().filter_map(|t| {
    Some([
      *p.pos.get(t[0] as usize)?,
      *p.pos.get(t[1] as usize)?,
      *p.pos.get(t[2] as usize)?,
    ])
  })
}

/// The point of triangle abc nearest to p (Ericson, Real-Time Collision Detection, 5.1.5)
pub(super) fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
  let (ab, ac, ap) = (b - a, c - a, p - a);
  let (d1, d2) = (dot(ab, ap), dot(ac, ap));
  if d1 <= 0.0 && d2 <= 0.0 {
    return a;
  }
  let bp = p - b;
  let (d3, d4) = (dot(ab, bp), dot(ac, bp));
  if d3 >= 0.0 && d4 <= d3 {
    return b;
  }
  let vc = d1 * d4 - d3 * d2;
  if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
    return a + ab * (d1 / (d1 - d3));
  }
  let cp = p - c;
  let (d5, d6) = (dot(ab, cp), dot(ac, cp));
  if d6 >= 0.0 && d5 <= d6 {
    return c;
  }
  let vb = d5 * d2 - d1 * d6;
  if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
    return a + ac * (d2 / (d2 - d6));
  }
  let va = d3 * d6 - d5 * d4;
  if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
    return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
  }
  let k = 1.0 / (va + vb + vc);
  a + ab * (vb * k) + ac * (vc * k)
}

/// Triangles of the game's space hashed on a grid of `cell` meters: the distance from a point to the
/// nearest, how many meet a box.
pub(super) struct TriGrid {
  cell: f32,
  tris: Vec<[Vec3; 3]>,
  boxes: Vec<AABB>,
  cells: HashMap<u64, Vec<u32>>,
  /// the last query that met each triangle
  seen: RefCell<Vec<u32>>,
  query: Cell<u32>,
}

impl TriGrid {
  pub(super) fn new(cell: f32) -> Self {
    Self {
      cell,
      tris: Vec::new(),
      boxes: Vec::new(),
      cells: HashMap::new(),
      seen: RefCell::new(Vec::new()),
      query: Cell::new(0),
    }
  }

  pub(super) fn key(x: i32, y: i32, z: i32) -> u64 {
    ((x.wrapping_add(1 << 20) as u32 as u64) << 42)
      | ((y.wrapping_add(1 << 20) as u32 as u64) << 21)
      | (z.wrapping_add(1 << 20) as u32 as u64)
  }

  pub(super) fn at(&self, v: f32) -> i32 {
    (v / self.cell).floor() as i32
  }

  pub(super) fn is_empty(&self) -> bool {
    self.tris.is_empty()
  }

  /// the cells a box covers
  pub(super) fn cells_of(&self, b: &AABB) -> impl Iterator<Item = u64> + '_ {
    let (lo, hi) = (b.lo, b.hi);
    (self.at(lo.x)..=self.at(hi.x)).flat_map(move |x| {
      (self.at(lo.y)..=self.at(hi.y))
        .flat_map(move |y| (self.at(lo.z)..=self.at(hi.z)).map(move |z| Self::key(x, y, z)))
    })
  }

  pub(super) fn add(&mut self, a: Vec3, b: Vec3, c: Vec3) {
    let mut bx = AABB::default();
    bx.add(a);
    bx.add(b);
    bx.add(c);
    let t = self.tris.len() as u32;
    self.tris.push([a, b, c]);
    self.boxes.push(bx);
    let side = |lo: f32, hi: f32| self.at(hi) as i64 - self.at(lo) as i64 + 1;
    let span = side(bx.lo.x, bx.hi.x) * side(bx.lo.y, bx.hi.y) * side(bx.lo.z, bx.hi.z);
    if span <= 64 {
      let keys: Vec<u64> = self.cells_of(&bx).collect();
      for k in keys {
        self.cells.entry(k).or_default().push(t);
      }
      return;
    }
    // a large one (a level's ground, a far mountain): the cells its surface crosses, not all the
    // cells of its box (a slanted triangle of a kilometer would fill millions)
    let longest = length(b - a).max(length(c - b)).max(length(a - c));
    let n = ((longest / (self.cell * 0.5)).ceil() as i32).clamp(1, 2048);
    let mut crossed = HashSet::new();
    for i in 0..=n {
      for j in 0..=(n - i) {
        let p = a + (b - a) * (i as f32 / n as f32) + (c - a) * (j as f32 / n as f32);
        crossed.insert(Self::key(self.at(p.x), self.at(p.y), self.at(p.z)));
      }
    }
    for k in crossed {
      self.cells.entry(k).or_default().push(t);
    }
  }

  /// f(t) once for each triangle in the cells the box covers
  pub(super) fn each_near(&self, bx: &AABB, mut f: impl FnMut(u32)) {
    let mut seen = self.seen.borrow_mut();
    if seen.len() < self.tris.len() {
      seen.resize(self.tris.len(), 0);
    }
    let query = self.query.get() + 1;
    self.query.set(query);
    for k in self.cells_of(bx) {
      let Some(ts) = self.cells.get(&k) else {
        continue;
      };
      for &t in ts {
        if seen[t as usize] != query {
          seen[t as usize] = query;
          f(t);
        }
      }
    }
  }

  /// the distance to the nearest triangle, `limit` when none is nearer
  pub(super) fn distance(&self, p: Vec3, limit: f32) -> f32 {
    let mut best = limit;
    let mut bx = AABB::default();
    bx.add(p - Vec3::new(limit, limit, limit));
    bx.add(p + Vec3::new(limit, limit, limit));
    self.each_near(&bx, |t| {
      let [a, b, c] = self.tris[t as usize];
      best = best.min(length(closest_on_triangle(p, a, b, c) - p));
    });
    best
  }

  pub(super) fn count(&self, bx: &AABB) -> usize {
    let mut n = 0;
    self.each_near(bx, |t| n += self.boxes[t as usize].overlaps(bx) as usize);
    n
  }

  /// at least count(box): the triangles of the cells it covers, a triangle over several cells
  /// counted in each (cheap)
  pub(super) fn bound(&self, bx: &AABB) -> usize {
    self
      .cells_of(bx)
      .filter_map(|k| self.cells.get(&k))
      .map(Vec::len)
      .sum()
  }
}

/// The level whose decor a node is part of: <level> of its outermost ancestor <level>-tfrag-*,
/// -tie-* or -shrub-* (the draw groups of an extracted level), and that group's parent (which copy
/// of the level, in a document); "" for none
pub(super) fn piece_level(scene: &Scene, n: &Node) -> (u32, String) {
  let mut found = (0, String::new());
  let mut guard = 0;
  let mut p = if n.parent != 0 {
    scene.find(n.parent)
  } else {
    None
  };
  while let Some(g) = p {
    if guard >= 1000 {
      break;
    }
    guard += 1;
    for kind in ["-tfrag-", "-tie-", "-shrub-"] {
      if let Some(at) = g.name.find(kind)
        && at > 0
      {
        found = (g.parent, g.name[..at].to_string());
      }
    }
    p = if g.parent != 0 {
      scene.find(g.parent)
    } else {
      None
    };
  }
  found
}

/// "<level>" of a group "<level>-collision" (the game's collision of a level)
pub(super) fn collision_group_level(name: &str) -> Option<&str> {
  name.strip_suffix("-collision").filter(|l| !l.is_empty())
}

/// The parts of triangle abc to keep, `gone(point)` saying where the collision has no piece left:
/// all of it, none, or (when its corners and its middle disagree) its quarters, each decided again,
/// down to 1 m
pub(super) fn keep_parts(
  a: Vec3,
  b: Vec3,
  c: Vec3,
  depth: i32,
  gone: &impl Fn(Vec3) -> bool,
  out: &mut impl FnMut(Vec3, Vec3, Vec3),
) {
  let middle = gone((a + b + c) * (1.0 / 3.0));
  let longest = length2(b - a).max(length2(c - b)).max(length2(a - c));
  if depth < 6 && longest > 1.0 && (gone(a) != middle || gone(b) != middle || gone(c) != middle) {
    let (ab, bc, ca) = ((a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5);
    keep_parts(a, ab, ca, depth + 1, gone, out);
    keep_parts(ab, b, bc, depth + 1, gone, out);
    keep_parts(ca, bc, c, depth + 1, gone, out);
    keep_parts(ab, bc, ca, depth + 1, gone, out);
    return;
  }
  if !middle {
    out(a, b, c);
  }
}

/// Jak 3 gathers the collision around Jak in a cache of 461 triangles (collide-cache-h.gc); beyond
/// it, the triangles of the place are dropped and Jak can fall through the floor. The box it fills,
/// around his feet: his root sphere (2.2 m, 1.2 m up) and 1 m more (fill-cache-for-shape).
pub(super) const K_COLLIDE_CACHE_TRIS: usize = 460;

/// the pieces of decor colliding by their shape are 4 m across at least (walls, floors, catwalks,
/// stairs); the smaller ones (unless given a collision in the editor, or shapes of the editor) none
pub(super) const K_SMALL_PIECE: f32 = 4.0;

pub(super) const K_CACHE_BOX_LO: Vec3 = Vec3::new(-3.2, -2.0, -3.2);

pub(super) const K_CACHE_BOX_HI: Vec3 = Vec3::new(3.2, 4.4, 3.2);

/// The places of the floors (the faces up, a point every 3 m) where the cache would get more
/// triangles than it holds (`sides`: 2 when the builder doubles every face): how many, and where,
/// the densest first
pub(super) fn collision_hot_spots(all: &TriGrid, sides: usize) -> Vec<(usize, Vec3)> {
  let mut floors: HashMap<u64, Vec3> = HashMap::new();
  let mut sample = |p: Vec3| {
    floors
      .entry(TriGrid::key(
        (p.x / 3.0).floor() as i32,
        p.y.floor() as i32,
        (p.z / 3.0).floor() as i32,
      ))
      .or_insert(p);
  };
  for &[a, b, c] in &all.tris {
    let n = cross(b - a, c - a);
    let len = length(n);
    if len <= 0.0 || n.y < 0.7 * len {
      continue; // facing up
    }
    let longest = length2(b - a)
      .max(length2(c - b))
      .max(length2(a - c))
      .sqrt();
    let steps = 64.min((longest / 3.0).ceil() as i32);
    if steps <= 1 {
      sample((a + b + c) * (1.0 / 3.0));
      continue;
    }
    for i in 0..=steps {
      for j in 0..=(steps - i) {
        sample(a + (b - a) * (i as f32 / steps as f32) + (c - a) * (j as f32 / steps as f32));
      }
    }
  }
  let mut out = Vec::new();
  for &at in floors.values() {
    let mut bx = AABB::default();
    bx.add(at + K_CACHE_BOX_LO);
    bx.add(at + K_CACHE_BOX_HI);
    if all.bound(&bx) * sides <= K_COLLIDE_CACHE_TRIS {
      continue;
    }
    let n = all.count(&bx) * sides;
    if n > K_COLLIDE_CACHE_TRIS {
      out.push((n, at));
    }
  }
  // the densest first; the same count by place, the order of a hash map being no order
  out.sort_by(|a, b| {
    b.0
      .cmp(&a.0)
      .then_with(|| a.1.x.total_cmp(&b.1.x))
      .then_with(|| a.1.y.total_cmp(&b.1.y))
      .then_with(|| a.1.z.total_cmp(&b.1.z))
  });
  out
}

/// The collision the element at `index` gets in the game when the editor sets none.
pub fn default_decor_collision(scene: &Scene, index: usize) -> DefaultCollision {
  let cache = scene.cache();
  let Some(mesh) = scene.nodes.get(index).and_then(|n| n.mesh.as_ref()) else {
    return DefaultCollision::Shape;
  };
  if index >= cache.bounds.len() {
    return DefaultCollision::Shape;
  }
  let n = &scene.nodes[index];
  // a piece of a level whose collision is in the document: its "<level>-collision" group, next to it
  let level = piece_level(scene, n);
  if !level.1.is_empty() {
    let group = format!("{}-collision", level.1);
    if scene
      .nodes
      .iter()
      .any(|g| g.kind == NodeKind::Group && g.parent == level.0 && g.name == group)
    {
      return DefaultCollision::Game;
    }
  }
  if see_through(scene, mesh) {
    return DefaultCollision::SeeThrough;
  }
  let b = &cache.bounds[index];
  if n.extras.get("og_shape").is_none() && b.valid() && max3(b.size()) < K_SMALL_PIECE {
    return DefaultCollision::Small;
  }
  DefaultCollision::Shape
}

pub(super) fn max3(v: Vec3) -> f32 {
  v.x.max(v.y).max(v.z)
}

/// Whether the node at index `i` is part of the document's own level: not under a level added to it,
/// a companion, a mission layer, nor a reference.
pub fn own_level_node(scene: &Scene, i: usize) -> bool {
  let c = scene.cache();
  if i >= scene.nodes.len() {
    return false;
  }
  let marked = |n: &Node| {
    n.kind == NodeKind::Group
      && n.extras.as_object().is_some_and(|e| {
        ["og_level", "og_companion", "og_layer", "og_reference"]
          .iter()
          .any(|k| e.contains_key(*k))
      })
  };
  if c.parent.len() == c.count && i < c.count {
    let mut k = i as i32;
    let mut guard = 0;
    while k >= 0 && guard < 1000 {
      if marked(&scene.nodes[k as usize]) {
        return false;
      }
      k = c.parent[k as usize];
      guard += 1;
    }
    return true;
  }
  let mut n = Some(&scene.nodes[i]);
  let mut guard = 0;
  while let Some(node) = n {
    if guard >= 1000 {
      break;
    }
    if marked(node) {
      return false;
    }
    n = if node.parent != 0 {
      scene.find(node.parent)
    } else {
      None
    };
    guard += 1;
  }
  true
}

/// The levels whose collision the document has (its "<level>-collision" groups: "jak2-<level>" for
/// a level of Jak II): the export compares their decor with the level as extracted
/// (LevelExportOptions::originals) when it keeps the game's collision. Each once, in order.
pub fn collision_levels(scene: &Scene) -> Vec<String> {
  let mut out: Vec<String> = Vec::new();
  for n in &scene.nodes {
    if n.kind == NodeKind::Group
      && let Some(level) = collision_group_level(&n.name)
      && !out.iter().any(|l| l == level)
    {
      out.push(level.to_string());
    }
  }
  out
}

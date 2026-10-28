//! Triangle strips of the .fr3 as triangle lists (the decompiler's fr3_to_gltf.cpp and
//! tfrag_tie_fixup.cpp, which ogle-extract wrote the library with): tfrag and TIE strips are made
//! to face one way, strip by strip, as their neighbors do.

use std::collections::{BTreeSet, HashMap};

const RESTART: u32 = u32::MAX;

/// Tfrag and TIE strips as triangles, each strip turned to agree with the strips sharing its edges
/// (tfrag_tie_fixup.cpp). `old_to_new[i]`: where the triangles from strip index `i` on start in the
/// result (exact at the start of a draw).
pub fn unstrip_tfrag_tie(strips: &[u32], positions: &[[f32; 3]]) -> (Vec<u32>, Vec<u32>) {
  // 1: the strips' triangles, every other one turned back
  let mut old_to_new = vec![0, 0];
  let mut groups: Vec<Vec<[u32; 3]>> = Vec::new();
  let mut building: Vec<[u32; 3]> = Vec::new();
  let mut toggle = false;
  let mut count = 0u32;
  for i in 2..strips.len() {
    let (a, b, c) = (strips[i], strips[i - 1], strips[i - 2]);
    old_to_new.push(count);
    if a == RESTART || b == RESTART || c == RESTART {
      toggle = false;
      if !building.is_empty() {
        groups.push(std::mem::take(&mut building));
      }
      continue;
    }
    count += 3;
    building.push(if toggle { [a, b, c] } else { [a, c, b] });
    toggle = !toggle;
  }
  old_to_new.push(count);

  // 2: the strips sharing an edge (vertices rounded to game units), and for each pair how many of
  // their shared edges go the same way (twisted) or not
  let round = |p: [f32; 3]| p.map(|x| (x as i64) as f32);
  let greater = |a: &[f32; 3], b: &[f32; 3]| {
    for k in 0..3 {
      if a[k] > b[k] {
        return true;
      }
      if a[k] < b[k] {
        return false;
      }
    }
    false
  };
  let mut edges: HashMap<[u32; 6], Vec<(usize, bool)>> = HashMap::new();
  for (g, tris) in groups.iter().enumerate() {
    for tri in tris {
      for e in 0..3 {
        let (p, q) = (
          positions[tri[e] as usize],
          positions[tri[(e + 1) % 3] as usize],
        );
        let (rp, rq) = (round(p), round(q));
        let (lo, hi) = if greater(&rp, &rq) {
          (rq, rp)
        } else {
          (rp, rq)
        };
        let key = [lo, hi]
          .concat()
          .iter()
          .map(|x| x.to_bits())
          .collect::<Vec<_>>();
        let list = edges.entry(key.try_into().unwrap()).or_default();
        if !list.iter().any(|&(s, _)| s == g) {
          list.push((g, greater(&p, &q)));
        }
      }
    }
  }
  let mut neighbors: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); groups.len()];
  // (lower group, higher group) -> (twisted, straight)
  let mut pairs: HashMap<(usize, usize), (i32, i32)> = HashMap::new();
  for infos in edges.values() {
    for i in 0..infos.len() {
      for j in 0..i {
        let ((a, oa), (b, ob)) = (infos[i], infos[j]);
        neighbors[a].insert(b);
        neighbors[b].insert(a);
        let e = pairs.entry((a.min(b), a.max(b))).or_default();
        if oa == ob {
          e.0 += 1;
        } else {
          e.1 += 1;
        }
      }
    }
  }

  // 3, 4: each connected part in breadth-first order from its lowest strip; 5: a strip is turned
  // when that agrees with more of the edges it shares with the strips decided before it
  let mut flips = vec![false; groups.len()];
  let mut seen = vec![false; groups.len()];
  let mut decided = vec![false; groups.len()];
  for start in 0..groups.len() {
    if seen[start] {
      continue;
    }
    let mut order = Vec::new();
    let mut frontier = vec![start];
    while !frontier.is_empty() {
      frontier.sort_by_key(|&x| neighbors[x].len()); // stable, as std::stable_sort
      let mut next = Vec::new();
      for x in frontier {
        if seen[x] {
          continue;
        }
        seen[x] = true;
        order.push(x);
        next.extend(neighbors[x].iter().copied().filter(|&n| !seen[n]));
      }
      frontier = next;
    }
    for &node in &order {
      let mut sum = 0;
      for &n in &neighbors[node] {
        if !decided[n] {
          continue;
        }
        let (twisted, straight) = pairs[&(node.min(n), node.max(n))];
        // a twist with a turned neighbor is straight
        let (t, s) = if flips[n] {
          (straight, twisted)
        } else {
          (twisted, straight)
        };
        sum += s - t;
      }
      flips[node] = sum < 0;
      decided[node] = true;
    }
  }

  // 6: the triangles, the turned strips' turned
  let mut out = Vec::with_capacity(count as usize);
  for (g, tris) in groups.iter().enumerate() {
    for t in tris {
      if flips[g] {
        out.extend([t[1], t[0], t[2]]);
      } else {
        out.extend(*t);
      }
    }
  }
  (out, old_to_new)
}

/// Strips of `stream[start..start + count]` as triangles (shrub and wind draws: no fixup)
pub fn unstrip_plain(stream: &[u32], start: usize, count: usize, out: &mut Vec<u32>) {
  for i in start + 2..start + count {
    let (a, b, c) = (stream[i], stream[i - 1], stream[i - 2]);
    if a != RESTART && b != RESTART && c != RESTART {
      out.extend([a, b, c]);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strips_unstripped_facing_one_way() {
    // two quads side by side, each a strip (ended by a restart, as the .fr3's always are), the
    // second facing the other way: it is turned to agree with the first
    let m = 4096.0;
    let p = [
      [0.0, 0.0, 0.0],
      [m, 0.0, 0.0],
      [0.0, 0.0, m],
      [m, 0.0, m],
      [2.0 * m, 0.0, 0.0],
      [2.0 * m, 0.0, m],
    ];
    let (tris, old_to_new) = unstrip_tfrag_tie(&[0, 1, 2, 3, RESTART, 4, 1, 5, 3, RESTART], &p);
    assert_eq!(tris, [2, 0, 1, 3, 2, 1, 4, 5, 1, 5, 3, 1]);
    let n = |t: &[u32]| {
      let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
      (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2])
    };
    assert!(tris.chunks(3).all(|t| n(t) < 0.0));
    assert_eq!(old_to_new[5], 6); // the second strip's first index
    let mut plain = Vec::new();
    unstrip_plain(&[0, 1, 2, RESTART, 1, 2, 3], 0, 7, &mut plain);
    assert_eq!(plain, [2, 1, 0, 3, 2, 1]);
  }
}

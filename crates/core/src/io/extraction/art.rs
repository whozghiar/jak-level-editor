//! The skeletons and animations of the models, from a level's art groups (`<name>-ag`, which
//! OpenGOAL's extractor dumps to raw_obj/): read as its decompiler reads them
//! (extract_joint_group.cpp, extract_anim.cpp) and decompressed as it writes them to glTF
//! (fr3_to_gltf.cpp, decompress_anim).
//!
//! A model gets the skeleton of its name (`<art>-lod0`) and the animations whose master art group
//! is its art (`<art>-lod0`, the lower levels of detail none), from any art group of its level.

use std::collections::HashMap;

use super::goal_object::{GoalObject, Word};
use super::lzo;

/// A bone: its parent's index (-1: none) and its bind pose as the game stores it (4 rows of 4
/// floats, the translation in game units)
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
  pub name: String,
  pub parent: i32,
  pub bind: [f32; 16],
}

/// A bone's keys, one per frame
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JointKeys {
  pub trans: Vec<[f32; 3]>,
  pub quat: Vec<[f32; 4]>,
  pub scale: Vec<[f32; 3]>,
}

/// An animation decompressed: the keys of bones 2.. (0 and 1, align and prejoint, have none), and
/// per frame the weights of the model's blend shapes (bytes, 64: none)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Anim {
  pub name: String,
  pub framerate: f32,
  pub frames: usize,
  pub joints: Vec<JointKeys>,
  pub blend: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ArtData {
  pub joints: Vec<Joint>,
  pub anims: Vec<Anim>,
}

/// By model name ("<art>-lod0")
pub type Art = HashMap<String, ArtData>;

/// Where the fields are (relative to the type tag), per game (all-types.gc)
struct Layout {
  master: u32,
  blend: u32,
  frames: u32,
  speed: u32,
  geo_data: u32,
}

fn layout(game: &str) -> Layout {
  match game {
    "jak2" => Layout {
      master: 32,
      blend: 40,
      frames: 44,
      speed: 20,
      geo_data: 32,
    },
    _ => Layout {
      master: 20,
      blend: 32,
      frames: 36,
      speed: 48,
      geo_data: 48,
    },
  }
}

/// The animations whose master art group is not their model's (extract_anim.cpp): they go to the
/// model their name starts with
const MODEL_OF_ANIM: [&str; 7] = [
  "collectables-bomb-blast",
  "collectables-health",
  "collectables-gem",
  "collectables-generic-blast",
  "collectables-generic-ripples",
  "collectables-skill",
  "collectables-warp-time",
];

/// Adds the skeletons and animations of an art group (its object file) to `art`
pub fn read_art_group(data: &[u8], game: &str, art: &mut Art) -> Result<(), String> {
  add_art(&GoalObject::read(data)?, game, art)
}

fn add_art(o: &GoalObject, game: &str, art: &mut Art) -> Result<(), String> {
  let l = layout(game);
  let tags = |ty: &str| -> Vec<u32> {
    let ty = Word::Type(ty.into());
    (0..o.words.len() as u32)
      .filter(|&i| o.words[i as usize] == ty)
      .map(|i| i * 4)
      .collect()
  };
  let string_at = |at: u32| o.ptr(at).and_then(|p| o.string(p)).unwrap_or_default();

  // skeletons (art-joint-geo): the joints, each pointing to its parent
  for geo in tags("art-joint-geo") {
    let name = string_at(geo + 8);
    let length = o.u32(geo + 12) as usize;
    let mut tag_of_joint = HashMap::new();
    let mut joints = Vec::with_capacity(length);
    for i in 0..length {
      let j = o
        .ptr(geo + l.geo_data + 4 * i as u32)
        .ok_or_else(|| format!("{name}: joint {i}"))?
        - 4;
      tag_of_joint.insert(j, i as i32);
      let parent = match o.ptr(j + 12) {
        Some(p) => *tag_of_joint
          .get(&(p - 4))
          .ok_or_else(|| format!("{name}: joint {i}'s parent"))?,
        None => -1,
      };
      joints.push(Joint {
        name: string_at(j + 4),
        parent,
        bind: std::array::from_fn(|k| o.f32(j + 16 + 4 * k as u32)),
      });
    }
    art.entry(name).or_default().joints = joints;
  }

  // animations (art-joint-anim)
  for t in tags("art-joint-anim") {
    let name = string_at(t + 8);
    let master = string_at(t + l.master);
    let Some(control) = o.ptr(t + l.frames) else {
      continue;
    };
    let mut anim = decompress(o, &name, o.f32(t + l.speed), control)?;
    if let Some(p) = o.ptr(t + l.blend) {
      anim.blend = plain_bytes(o, p);
    }
    let model = MODEL_OF_ANIM
      .iter()
      .find(|m| name.contains(*m))
      .map_or(master, |m| m.to_string());
    art
      .entry(format!("{model}-lod0"))
      .or_default()
      .anims
      .push(anim);
  }
  Ok(())
}

/// The plain data words from `at` on, as bytes (get_plain_data_bytes_up_to_label)
fn plain_bytes(o: &GoalObject, at: u32) -> Vec<u8> {
  let mut out = Vec::new();
  for w in o.words.iter().skip(at as usize / 4) {
    match w {
      Word::Data(d) => out.extend_from_slice(&d.to_le_bytes()),
      _ => break,
    }
  }
  out
}

/// A frame's (or the fixed part's) data: 64, 32 and 16 bits values, then matrices
#[derive(Default)]
struct Packed {
  d64: Vec<u8>,
  d32: Vec<u8>,
  d16: Vec<u8>,
}

/// After `header` bytes: offsets of the 64, 32 and 16 bits data (relative to after the 16 bytes of
/// offsets), the 16 bits running to the end of `qwc` quadwords (less `fixed_head` of them)
fn packed(b: &[u8], header: usize, qwc: u32, fixed_head: u32) -> Packed {
  let u = |o: usize| {
    b.get(o..o + 4)
      .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
  };
  let (o64, o32, o16) = (u(header), u(header + 4), u(header + 8));
  let s64 = o32.wrapping_sub(o64) as usize;
  let s32 = o16.wrapping_sub(o32) as usize;
  let s16 = (qwc.saturating_sub(fixed_head) as usize * 16).saturating_sub(s64 + s32);
  let data = header + 16;
  let take = |off: u32, n: usize| {
    let start = data + off as usize;
    (0..n)
      .map(|i| b.get(start + i).copied().unwrap_or(0))
      .collect()
  };
  Packed {
    d64: take(o64, s64),
    d32: take(o32, s32),
    d16: take(o16, s16),
  }
}

/// Reads values from one of the data streams, zeros past its end
struct Stream<'a> {
  b: &'a [u8],
  at: usize,
}

impl Stream<'_> {
  fn skip(&mut self, n: usize) {
    self.at += n;
  }
  fn bytes<const N: usize>(&mut self) -> [u8; N] {
    let v = std::array::from_fn(|i| self.b.get(self.at + i).copied().unwrap_or(0));
    self.at += N;
    v
  }
  fn f32(&mut self) -> f32 {
    f32::from_le_bytes(self.bytes())
  }
  fn s16(&mut self) -> f32 {
    i16::from_le_bytes(self.bytes()) as f32
  }
}

const QUAT_SCALE: f32 = 1.0 / 32768.0;
const SCALE_SCALE: f32 = 1.0 / 4096.0;
const TRANS_SCALE: f32 = 4.0 / 4096.0;

/// An animation's keys from its joint-anim-compressed-control at `control`
fn decompress(o: &GoalObject, name: &str, speed: f32, control: u32) -> Result<Anim, String> {
  let first = o.u32(control);
  let num_frames = (first & 0xffff) as usize;
  let (fixed_qwc, frame_qwc) = (o.u32(control + 4), o.u32(control + 8));
  let fixed_at = o
    .ptr(control + 12)
    .ok_or_else(|| format!("{name}: no fixed data"))?;
  // the fixed part: 14 words of control bits, the joints, the matrix bits, then its data
  let (fixed, frames): (Vec<u8>, Vec<Vec<u8>>) = if (first >> 16) & 1 != 0 {
    // compressed with LZO, the frames after the fixed part
    let mut out = vec![0u8; (fixed_qwc as usize + num_frames * frame_qwc as usize) * 16];
    lzo::decompress(&plain_bytes(o, fixed_at), &mut out).map_err(|e| format!("{name}: {e}"))?;
    let base = fixed_qwc as usize * 16;
    let step = frame_qwc as usize * 16;
    let frames = (0..num_frames)
      .map(|i| out[base + i * step..base + (i + 1) * step].to_vec())
      .collect();
    (out[..base].to_vec(), frames)
  } else {
    let frames = (0..num_frames)
      .map(|i| {
        let at = o.ptr(control + 16 + 4 * i as u32).unwrap_or(0);
        o.bytes(at, frame_qwc as usize * 16)
      })
      .collect();
    (o.bytes(fixed_at, fixed_qwc as usize * 16), frames)
  };
  let u = |b: &[u8], at: usize| {
    b.get(at..at + 4)
      .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
  };
  let control_bits: [u32; 14] = std::array::from_fn(|i| u(&fixed, i * 4));
  let num_joints = u(&fixed, 56) as usize;
  let matrix_bits = u(&fixed, 60);
  // a matrix of the fixed part, else of every frame
  let mat = [matrix_bits & 1 == 0, matrix_bits & 2 == 0];
  let fixed = packed(&fixed, 64, fixed_qwc, 5);
  let frames: Vec<Packed> = frames.iter().map(|f| packed(f, 0, frame_qwc, 1)).collect();

  let mut joints = vec![JointKeys::default(); num_joints];
  let bits = |j: usize| (control_bits.get(j / 8).copied().unwrap_or(0) >> (4 * (j % 8))) & 0xf;
  let key = |c: u32,
             animated: bool,
             s64: &mut Stream,
             s32: &mut Stream,
             s16: &mut Stream,
             k: &mut JointKeys| {
    // c: bit 0 translation, 1 rotation, 2 scale in the frames (else once in the fixed part); bit
    // 3 a big translation (floats)
    if (c & 1 != 0) == animated {
      k.trans.push(if c & 8 != 0 {
        [s64.f32() / 4096.0, s64.f32() / 4096.0, s32.f32() / 4096.0]
      } else {
        [
          s32.s16() * TRANS_SCALE,
          s32.s16() * TRANS_SCALE,
          s16.s16() * TRANS_SCALE,
        ]
      });
    }
    if (c & 2 != 0) == animated {
      k.quat.push([
        s64.s16() * QUAT_SCALE,
        s64.s16() * QUAT_SCALE,
        s64.s16() * QUAT_SCALE,
        s64.s16() * QUAT_SCALE,
      ]);
    }
    if (c & 4 != 0) == animated {
      k.scale.push([
        s32.s16() * SCALE_SCALE,
        s32.s16() * SCALE_SCALE,
        s16.s16() * SCALE_SCALE,
      ]);
    }
  };
  {
    let (mut s64, mut s32, mut s16) = (
      Stream {
        b: &fixed.d64,
        at: 0,
      },
      Stream {
        b: &fixed.d32,
        at: 0,
      },
      Stream {
        b: &fixed.d16,
        at: 0,
      },
    );
    s64.skip(64 * mat.iter().filter(|&&m| m).count());
    for (j, k) in joints.iter_mut().enumerate() {
      key(bits(j), false, &mut s64, &mut s32, &mut s16, k);
    }
  }
  for f in &frames {
    let (mut s64, mut s32, mut s16) = (
      Stream { b: &f.d64, at: 0 },
      Stream { b: &f.d32, at: 0 },
      Stream { b: &f.d16, at: 0 },
    );
    s64.skip(64 * mat.iter().filter(|&&m| !m).count());
    for (j, k) in joints.iter_mut().enumerate() {
      key(bits(j), true, &mut s64, &mut s32, &mut s16, k);
    }
  }
  // what a bone does not animate stays as the fixed part has it (or at rest)
  fn fill<T: Copy>(v: &mut Vec<T>, frames: usize, rest: T) {
    let first = v.first().copied().unwrap_or(rest);
    v.resize(frames.max(v.len()), first);
  }
  for k in &mut joints {
    fill(&mut k.trans, num_frames, [0.0; 3]);
    fill(&mut k.quat, num_frames, [0.0, 0.0, 0.0, 1.0]);
    fill(&mut k.scale, num_frames, [1.0; 3]);
  }
  Ok(Anim {
    name: name.to_string(),
    framerate: if speed > 0.0 { speed * 60.0 } else { 30.0 },
    frames: num_frames,
    joints,
    blend: Vec::new(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn skeletons_and_animations_read() {
    // words: an art-joint-geo of 2 joints, the joints, an art-joint-anim of 2 frames, its data,
    // the strings
    let mut w = vec![Word::Data(0); 160];
    let ptr = |w: &mut Vec<Word>, at: usize, dest: u32| w[at / 4] = Word::Ptr(dest);
    let string = |w: &mut Vec<Word>, at: usize, s: &str| {
      // a string basic at `at` (its tag before it): its size, then its characters
      w[at / 4 - 1] = Word::Type("string".into());
      w[at / 4] = Word::Data(s.len() as u32);
      for (i, c) in s.as_bytes().chunks(4).enumerate() {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        w[at / 4 + 1 + i] = Word::Data(u32::from_le_bytes(b));
      }
    };
    // the skeleton at 0 (Jak 3): its name, 2 joints listed at 48
    w[0] = Word::Type("art-joint-geo".into());
    ptr(&mut w, 8, 524);
    string(&mut w, 524, "box-lod0");
    w[3] = Word::Data(2);
    // the joints at 100 and 180, the second the child of the first
    for (i, tag) in [100usize, 180].into_iter().enumerate() {
      ptr(&mut w, 48 + 4 * i, tag as u32 + 4);
      w[tag / 4] = Word::Type("joint".into());
      ptr(&mut w, tag + 4, 544 + 20 * i as u32);
      w[tag / 4 + 4] = Word::Data(1.0f32.to_bits()); // bind pose [0]
    }
    string(&mut w, 544, "a");
    string(&mut w, 564, "b");
    ptr(&mut w, 180 + 12, 104);
    // the animation at 260: its name, master "box", speed 0.5, its control at 320
    w[65] = Word::Type("art-joint-anim".into());
    ptr(&mut w, 260 + 8, 584);
    string(&mut w, 584, "box-idle");
    ptr(&mut w, 260 + 20, 604);
    string(&mut w, 604, "box");
    w[(260 + 48) / 4] = Word::Data(0.5f32.to_bits());
    ptr(&mut w, 260 + 36, 320);
    // the control: 2 frames, its fixed part (6 quadwords) at 400, its frames (2) past the object
    w[80] = Word::Data(2);
    w[81] = Word::Data(6);
    w[82] = Word::Data(2);
    ptr(&mut w, 332, 400);
    ptr(&mut w, 336, 900);
    ptr(&mut w, 340, 932);
    w[100] = Word::Data(0b0001); // joint 0: its translation per frame
    w[114] = Word::Data(1); // 1 joint
    w[115] = Word::Data(3); // no matrix
    // the data's offsets: 64 bits at 0 (the rotation), 32 at 8 (scale x, y), 16 at 12 (scale z)
    w[116] = Word::Data(0);
    w[117] = Word::Data(8);
    w[118] = Word::Data(12);
    w[121] = Word::Data(16384 << 16); // w = 0.5
    w[122] = Word::Data(4096 | (4096 << 16));
    w[123] = Word::Data(4096);
    let o = GoalObject::from_words(w);
    let mut art = Art::new();
    add_art(&o, "jak3", &mut art).unwrap();
    let box_ = &art["box-lod0"];
    assert_eq!(
      box_
        .joints
        .iter()
        .map(|j| (j.name.as_str(), j.parent))
        .collect::<Vec<_>>(),
      [("a", -1), ("b", 0)]
    );
    assert_eq!(box_.joints[0].bind[0], 1.0);
    let a = &box_.anims[0];
    assert_eq!(
      (a.name.as_str(), a.frames, a.framerate),
      ("box-idle", 2, 30.0)
    );
    assert_eq!(a.joints[0].quat, [[0.0, 0.0, 0.0, 0.5]; 2]);
    assert_eq!(a.joints[0].scale, [[1.0; 3]; 2]);
    // the frames' translations: past the object, zeros
    assert_eq!(a.joints[0].trans, [[0.0; 3]; 2]);
  }
}

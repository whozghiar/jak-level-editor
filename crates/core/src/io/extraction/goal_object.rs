//! A data object of the game as its DGO holds it (OpenGOAL's extractor dumps them to
//! `decompiler_out/<game>/raw_obj/<name>.go`): its words and what the game's linker makes of them,
//! pointers within the object, symbols, types, the empty list (the decompiler's
//! LinkedObjectFileCreation.cpp). Data objects only (one segment): link format 4 (Jak II) and 5
//! (Jak 3). Read for a level's actors and regions (bsp.rs).

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Word {
  Data(u32),
  /// a pointer to this byte offset of the object
  Ptr(u32),
  Symbol(String),
  Type(String),
  EmptyList,
  /// the offset of a symbol in the symbol table, in the lower 16 bits
  SymbolOffset(String),
}

pub struct GoalObject {
  pub words: Vec<Word>,
  /// the offsets pointed to, in order: the decompiler names them L1, L2...
  targets: BTreeSet<u32>,
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
  d.get(o..o + 4)
    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    .ok_or_else(|| format!("truncated at {o}"))
}
fn byte(d: &[u8], o: usize) -> Result<u8, String> {
  d.get(o).copied().ok_or_else(|| format!("truncated at {o}"))
}
fn cstr(d: &[u8], o: usize) -> Result<String, String> {
  let end = d[o.min(d.len())..]
    .iter()
    .position(|&c| c == 0)
    .ok_or("unterminated name")?;
  Ok(String::from_utf8_lossy(&d[o..o + end]).into_owned())
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
  Symbol,
  Type,
  EmptyList,
}

impl GoalObject {
  pub fn read(data: &[u8]) -> Result<Self, String> {
    let version = u32_at(data, 8)? & 0xffff;
    let mut obj = GoalObject {
      words: Vec::new(),
      targets: BTreeSet::new(),
    };
    match version {
      5 => obj.link_v5(data)?,
      4 => obj.link_v4(data)?,
      v => return Err(format!("link format {v}")),
    }
    Ok(obj)
  }

  fn push_words(&mut self, data: &[u8], start: usize, size: usize) -> Result<(), String> {
    for o in (start..start + size.div_ceil(4) * 4).step_by(4) {
      self.words.push(Word::Data(u32_at(data, o)?));
    }
    Ok(())
  }

  fn pointer(&mut self, at: usize, dest: u32) {
    // the art groups' links past the end are bogus: the decompiler leaves them
    if at / 4 < self.words.len() && dest as usize / 4 <= self.words.len() {
      self.words[at / 4] = Word::Ptr(dest);
      self.targets.insert(dest);
    }
  }

  /// The references to one symbol: seeks encoded on 1 to 4 bytes from `base` (c_symlink2)
  fn symbol_links(
    &mut self,
    data: &[u8],
    base: usize,
    mut lp: usize,
    kind: Kind,
    name: &str,
  ) -> Result<usize, String> {
    let mut at = base;
    loop {
      let b0 = byte(data, lp)? as u32;
      let mut seek = b0;
      let mut next = lp + 1;
      if seek & 3 != 0 {
        seek |= (byte(data, lp + 1)? as u32) << 8;
        next = lp + 2;
        if seek & 2 != 0 {
          seek |= (byte(data, lp + 2)? as u32) << 16;
          next = lp + 3;
          if seek & 1 != 0 {
            seek |= (byte(data, lp + 3)? as u32) << 24;
            next = lp + 4;
          }
        }
      }
      lp = next;
      at += (seek & !3) as usize;
      let i = (at - base) / 4;
      let word = if u32_at(data, at)? == u32::MAX {
        match kind {
          Kind::Symbol => Word::Symbol(name.into()),
          Kind::Type => Word::Type(name.into()),
          Kind::EmptyList => Word::EmptyList,
        }
      } else {
        Word::SymbolOffset(name.into())
      };
      if i < self.words.len() {
        self.words[i] = word;
      }
      if byte(data, lp)? == 0 {
        return Ok(lp + 1);
      }
    }
  }

  /// Jak 3: a header, the data at 0x80, the link data after it (link_v5, one segment)
  fn link_v5(&mut self, data: &[u8]) -> Result<(), String> {
    let to_code = u32_at(data, 4)? as usize;
    let to_link = u32_at(data, 12)? as usize;
    if byte(data, 20)? != 1 {
      return Err("not a data object".into());
    }
    let (relocs, seg_data, size) = (
      u32_at(data, to_link)? as usize,
      u32_at(data, to_link + 4)? as usize,
      u32_at(data, to_link + 8)? as usize,
    );
    let base = to_code + seg_data;
    self.push_words(data, base, size)?;
    let mut lp = to_link + relocs;
    // pointers: seek, fix count, seek... (0xff continues a count, 0xff 0 switches)
    let mut dp = base as isize - 4;
    let mut fixing = false;
    if byte(data, lp)? != 0 {
      loop {
        loop {
          let n = byte(data, lp)?;
          if !fixing {
            dp += 4 * n as isize;
          } else {
            for _ in 0..n {
              let code = u32_at(data, dp as usize)?;
              if code >> 24 == 0 {
                self.pointer(dp as usize - base, code);
              }
              dp += 4;
            }
          }
          if n != 0xff {
            break;
          }
          lp += 1;
          if byte(data, lp)? == 0 {
            lp += 1;
            fixing = !fixing;
          }
        }
        lp += 1;
        fixing = !fixing;
        if byte(data, lp)? == 0 {
          break;
        }
      }
    }
    lp += 1;
    // symbols and types
    if byte(data, lp)? != 0 {
      let mut sub = lp;
      loop {
        let reloc = byte(data, sub)?;
        if reloc & 0x80 != 0 && reloc & 0x3f == 0x3f {
          return Err("unknown link".into());
        }
        lp = sub + 3;
        let name = cstr(data, lp)?;
        lp += name.len() + 1;
        let kind = if reloc & 0x80 != 0 {
          Kind::Type
        } else if name == "_empty_" {
          Kind::EmptyList
        } else {
          Kind::Symbol
        };
        lp = self.symbol_links(data, base, lp, kind, &name)?;
        sub = lp;
        if byte(data, sub)? == 0 {
          break;
        }
      }
    }
    Ok(())
  }

  /// Jak II: a header, the data, then link data of format 2 (link_v2_or_v4)
  fn link_v4(&mut self, data: &[u8]) -> Result<(), String> {
    let code_size = u32_at(data, 12)? as usize;
    let base = 16;
    self.push_words(data, base, code_size)?;
    let mut lp = base + code_size + 12;
    if byte(data, lp)? == 0 {
      lp += 1;
    } else {
      let mut cp = base;
      let mut fixing = false;
      loop {
        loop {
          let n = byte(data, lp)?;
          lp += 1;
          if !fixing {
            cp += 4 * n as usize;
          } else {
            for _ in 0..n {
              let dest = u32_at(data, cp)?;
              self.pointer(cp - base, dest);
              cp += 4;
            }
          }
          if n != 0xff {
            break;
          }
          if byte(data, lp)? == 0 {
            lp += 1;
            fixing = !fixing;
          }
        }
        fixing = !fixing;
        if byte(data, lp)? == 0 {
          break;
        }
      }
      lp += 1;
    }
    if byte(data, lp)? != 0 {
      loop {
        let reloc = byte(data, lp)?;
        lp += 1;
        let mut kind = if reloc & 0x80 == 0 {
          lp -= 1; // a symbol's name starts at its first byte
          Kind::Symbol
        } else {
          Kind::Type
        };
        let name = cstr(data, lp)?;
        if name == "_empty_" {
          kind = Kind::EmptyList;
        }
        lp += name.len() + 1;
        lp = self.symbol_links(data, base, lp, kind, &name)?;
        if byte(data, lp)? == 0 {
          break;
        }
      }
    }
    Ok(())
  }

  /// An object of these words (tests)
  #[cfg(test)]
  pub fn from_words(words: Vec<Word>) -> Self {
    let targets = words
      .iter()
      .filter_map(|w| match w {
        Word::Ptr(p) => Some(*p),
        _ => None,
      })
      .collect();
    GoalObject { words, targets }
  }

  // --------------------------------------------------------------------------------------------
  // reading
  // --------------------------------------------------------------------------------------------

  pub fn word(&self, byte_offset: u32) -> Option<&Word> {
    if !byte_offset.is_multiple_of(4) {
      return None;
    }
    self.words.get(byte_offset as usize / 4)
  }
  pub fn u32(&self, byte_offset: u32) -> u32 {
    match self.word(byte_offset & !3) {
      Some(Word::Data(d)) => *d,
      _ => 0,
    }
  }
  pub fn i16(&self, byte_offset: u32) -> i16 {
    let w = self.u32(byte_offset & !3);
    (if !byte_offset.is_multiple_of(4) {
      w >> 16
    } else {
      w & 0xffff
    }) as i16
  }
  pub fn f32(&self, byte_offset: u32) -> f32 {
    f32::from_bits(self.u32(byte_offset))
  }
  /// `n` bytes of plain data from `byte_offset`
  pub fn bytes(&self, byte_offset: u32, n: usize) -> Vec<u8> {
    (0..n)
      .map(|i| {
        let o = byte_offset + i as u32;
        (self.u32(o & !3) >> (8 * (o % 4))) as u8
      })
      .collect()
  }
  /// The offset a pointer word points to
  pub fn ptr(&self, byte_offset: u32) -> Option<u32> {
    match self.word(byte_offset)? {
      Word::Ptr(p) => Some(*p),
      _ => None,
    }
  }
  pub fn symbol(&self, byte_offset: u32) -> Option<&str> {
    match self.word(byte_offset)? {
      Word::Symbol(s) => Some(s),
      _ => None,
    }
  }
  pub fn type_name(&self, byte_offset: u32) -> Option<&str> {
    match self.word(byte_offset)? {
      Word::Type(s) => Some(s),
      _ => None,
    }
  }
  /// The type of the basic at `basic` (the word before it)
  pub fn type_of(&self, basic: u32) -> Option<&str> {
    basic.checked_sub(4).and_then(|t| self.type_name(t))
  }
  /// The decompiler's name of the label at `offset`: L<n>, in the order of the offsets
  pub fn label_name(&self, offset: u32) -> String {
    format!("L{}", self.targets.range(..offset).count() + 1)
  }
  /// A GOAL string at `basic` (its length, then its characters), as the decompiler reads it
  pub fn string(&self, basic: u32) -> Option<String> {
    let len = match self.word(basic)? {
      Word::Data(n) => *n as usize,
      _ => return None,
    };
    let b = self.bytes(basic + 4, len);
    if b.contains(&0) {
      return None;
    }
    Some(String::from_utf8_lossy(&b).into_owned())
  }

  /// The list at the pair `pair_offset` (offset & 7 == 2) as text: `quote` forms as 'x when
  /// `quotes` (the decompiler's pretty printer, but on one line), else as (quote x)
  pub fn list(&self, pair_offset: u32, quotes: bool) -> String {
    let mut items = Vec::new();
    let mut tail = None;
    let mut p = pair_offset;
    loop {
      items.push(self.list_object(p - 2, quotes));
      let cdr = p + 2;
      match self.word(cdr) {
        Some(Word::EmptyList) => break,
        Some(Word::Ptr(next)) if next & 7 == 2 && items.len() < 100_000 => p = *next,
        _ => {
          tail = Some(self.list_object(cdr, quotes));
          break;
        }
      }
    }
    if quotes && items.len() == 2 && items[0] == "quote" && tail.is_none() {
      return format!("'{}", items[1]);
    }
    match tail {
      Some(t) => format!("({} . {t})", items.join(" ")),
      None => format!("({})", items.join(" ")),
    }
  }

  /// An element of a list (to_form_script_object, as ogle-extract fixed it: a fixnum's value, a
  /// bfloat's)
  fn list_object(&self, at: u32, quotes: bool) -> String {
    match self.word(at) {
      Some(Word::Symbol(s)) => s.clone(),
      Some(Word::Data(d)) => {
        if d & 7 == 0 {
          ((*d as i32) >> 3).to_string()
        } else {
          d.to_string()
        }
      }
      Some(Word::Ptr(offset)) => {
        let offset = *offset;
        if offset & 7 == 2 {
          return self.list(offset, quotes);
        }
        if offset % 4 == 0 && self.type_of(offset) == Some("bfloat") {
          let f = self.f32(offset);
          let mut text = format_float(f);
          if !text.contains(['.', 'e', 'n']) {
            text += ".0";
          }
          return text;
        }
        if offset % 4 == 0 {
          let string = self.type_of(offset) == Some("string") && !self.after_empty_array(offset);
          if let Some(s) = self.string(offset).filter(|_| string) {
            return format!("\"{s}\"");
          }
        }
        self.label_name(offset)
      }
      Some(Word::EmptyList) => "()".into(),
      _ => "?".into(),
    }
  }

  // a string's type tag right after an empty array is not a string (is_string)
  fn after_empty_array(&self, basic: u32) -> bool {
    let t = basic - 4;
    t >= 12
      && self.type_name(t - 12) == Some("array")
      && self.word(t - 8) == Some(&Word::Data(0))
      && self.word(t - 4) == Some(&Word::Data(0))
  }
}

/// A float as fmt's "{}" prints it: the shortest text that reads back the same (ponytail: fmt
/// writes 1e+20 where Rust writes every digit; no list of the game holds such a float)
pub fn format_float(f: f32) -> String {
  format!("{f}")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lists_printed_as_the_decompiler() {
    // a list (want-load 'a 3 1.5): pairs at 8 and 16 bytes (car, cdr), its last cdr '()
    let mut o = GoalObject {
      words: vec![Word::Data(0); 16],
      targets: BTreeSet::new(),
    };
    let pair = |o: &mut GoalObject, at: usize, car: Word, cdr: Word| {
      o.words[at / 4] = car;
      o.words[at / 4 + 1] = cdr;
    };
    // the quote form (quote a) at 32
    pair(&mut o, 32, Word::Symbol("quote".into()), Word::Ptr(42));
    pair(&mut o, 40, Word::Symbol("a".into()), Word::EmptyList);
    // a bfloat 1.5 at 52 (its type at 48)
    o.words[12] = Word::Type("bfloat".into());
    o.words[13] = Word::Data(1.5f32.to_bits());
    pair(&mut o, 0, Word::Symbol("want-load".into()), Word::Ptr(10));
    pair(&mut o, 8, Word::Ptr(34), Word::Ptr(18));
    pair(&mut o, 16, Word::Data(3 << 3), Word::Ptr(26));
    pair(&mut o, 24, Word::Ptr(52), Word::EmptyList);
    assert_eq!(o.list(2, true), "(want-load 'a 3 1.5)");
    assert_eq!(o.list(2, false), "(want-load (quote a) 3 1.5)");
    assert_eq!(format_float(2.0), "2");
    assert_eq!(format_float(0.1), "0.1");
  }
}

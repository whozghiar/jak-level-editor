//! LZO1X decompression, for the animations Jak II and Jak 3 store compressed (the decompiler reads
//! them with lzokay, MIT license, Jack Andersen: third-party/lzokay/lzokay.cpp, decompress).

/// Decompresses `src` into `out` (its expected size): the bytes written. Input left after the end
/// marker is allowed, as the decompiler allows it.
pub fn decompress(src: &[u8], out: &mut [u8]) -> Result<usize, String> {
  let (mut i, mut o) = (0usize, 0usize);
  let need_in = |i: usize, n: usize| {
    if i + n > src.len() {
      Err("input overrun".to_string())
    } else {
      Ok(())
    }
  };
  let out_len = out.len();
  let need_out = |o: usize, n: usize| {
    if o + n > out_len {
      Err("output overrun".to_string())
    } else {
      Ok(())
    }
  };
  // a length continued by zero bytes (255 each) and a last byte
  let zero_run = |i: &mut usize| -> Result<usize, String> {
    let start = *i;
    while src.get(*i) == Some(&0) {
      *i += 1;
    }
    need_in(*i, 1)?;
    let n = (*i - start) * 255 + src[*i] as usize;
    *i += 1;
    Ok(n)
  };
  if src.len() < 3 {
    return Err("input overrun".into());
  }
  let mut state = 0usize;
  // the first byte: literals
  if src[0] >= 22 {
    let len = src[0] as usize - 17;
    i = 1;
    need_in(i, len)?;
    need_out(o, len)?;
    out[o..o + len].copy_from_slice(&src[i..i + len]);
    (i, o, state) = (i + len, o + len, 4);
  } else if src[0] >= 18 {
    let len = src[0] as usize - 17;
    i = 1;
    need_in(i, len)?;
    need_out(o, len)?;
    out[o..o + len].copy_from_slice(&src[i..i + len]);
    (i, o, state) = (i + len, o + len, len);
  }
  let mut lblen;
  loop {
    need_in(i, 1)?;
    let inst = src[i] as usize;
    i += 1;
    // the copy: `back` bytes behind the output, `lblen` of them, then `nstate` literals
    let (back, nstate);
    if inst & 0xc0 != 0 {
      // M2: 3 to 8 bytes within 2 kB
      need_in(i, 1)?;
      back = ((src[i] as usize) << 3) + ((inst >> 2) & 7) + 1;
      i += 1;
      lblen = (inst >> 5) + 1;
      nstate = inst & 3;
    } else if inst & 0x20 != 0 {
      // M3: within 16 kB
      lblen = (inst & 0x1f) + 2;
      if lblen == 2 {
        lblen += zero_run(&mut i)? + 31;
      }
      need_in(i, 2)?;
      let d = u16::from_le_bytes([src[i], src[i + 1]]) as usize;
      i += 2;
      back = (d >> 2) + 1;
      nstate = d & 3;
    } else if inst & 0x10 != 0 {
      // M4: within 16 to 48 kB; the end of the stream at 16384
      lblen = (inst & 7) + 2;
      if lblen == 2 {
        lblen += zero_run(&mut i)? + 7;
      }
      need_in(i, 2)?;
      let d = u16::from_le_bytes([src[i], src[i + 1]]) as usize;
      i += 2;
      let far = ((inst & 8) << 11) + (d >> 2);
      if far == 0 {
        break;
      }
      back = far + 16384;
      nstate = d & 3;
    } else if state == 0 {
      // a long literal run
      let mut len = inst + 3;
      if len == 3 {
        len += zero_run(&mut i)? + 15;
      }
      need_in(i, len)?;
      need_out(o, len)?;
      out[o..o + len].copy_from_slice(&src[i..i + len]);
      (i, o, state) = (i + len, o + len, 4);
      continue;
    } else {
      // M1: 2 bytes within 1 kB after 1 to 3 literals, 3 bytes 2 to 3 kB back after more
      need_in(i, 1)?;
      let base = (inst >> 2) + ((src[i] as usize) << 2);
      i += 1;
      (back, lblen) = if state != 4 {
        (base + 1, 2)
      } else {
        (base + 2049, 3)
      };
      nstate = inst & 3;
    }
    if back > o {
      return Err("lookbehind overrun".into());
    }
    need_in(i, nstate)?;
    need_out(o, lblen + nstate)?;
    // byte by byte: the copy can overlap what it writes
    for k in 0..lblen {
      out[o + k] = out[o + k - back];
    }
    o += lblen;
    out[o..o + nstate].copy_from_slice(&src[i..i + nstate]);
    (i, o, state) = (i + nstate, o + nstate, nstate);
  }
  if lblen != 3 {
    return Err("no end marker".into());
  }
  Ok(o)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn literals_and_copies_decompressed() {
    // 3 literals "abc", then an M2 copy of 3 bytes 3 back and 1 literal "e", then the end (M4)
    let src = [
      20,
      b'a',
      b'b',
      b'c',
      0x40 | (2 << 2) | 1,
      0,
      b'e',
      0x11,
      0,
      0,
    ];
    let mut out = [0u8; 7];
    assert_eq!(decompress(&src, &mut out), Ok(7));
    assert_eq!(&out, b"abcabce");
    // the copy overlapping its output repeats it
    let src = [18, b'x', 0x40 | 1 << 5, 0, 0x11, 0, 0];
    let mut out = [0u8; 5];
    assert_eq!(decompress(&src, &mut out), Ok(5));
    assert_eq!(&out, b"xxxxx");
    assert!(decompress(&[21, b'a'], &mut out).is_err());
  }
}

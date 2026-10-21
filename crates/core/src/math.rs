//! Small self-contained 3D math library: vectors, quaternions, column-major 4x4 matrices (OpenGL
//! convention), TRS transforms, bounding boxes and ray tests. Units are meters, like glTF.
//!
//! A port of the C++ editor's `core/math.h` with the same semantics: `fmin`/`fmax` are
//! `std::min`/`std::max` (the first argument when they compare equal or with a NaN), unlike
//! `f32::min`/`f32::max`.

use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

pub const K_PI: f32 = std::f32::consts::PI;

pub fn radians(deg: f32) -> f32 {
  deg * K_PI / 180.0
}

pub fn degrees(rad: f32) -> f32 {
  rad * 180.0 / K_PI
}

/// `std::min`: `a` unless `b < a`.
#[inline]
pub fn fmin(a: f32, b: f32) -> f32 {
  if b < a { b } else { a }
}

/// `std::max`: `a` unless `a < b`.
#[inline]
pub fn fmax(a: f32, b: f32) -> f32 {
  if a < b { b } else { a }
}

// ------------------------------------------------------------------------------------------------
// vectors
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "[f32; 2]", into = "[f32; 2]")]
pub struct Vec2 {
  pub x: f32,
  pub y: f32,
}

impl Vec2 {
  pub const fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
  pub fn length(&self) -> f32 {
    (self.x * self.x + self.y * self.y).sqrt()
  }
}

impl From<[f32; 2]> for Vec2 {
  fn from(a: [f32; 2]) -> Self {
    Self::new(a[0], a[1])
  }
}
impl From<Vec2> for [f32; 2] {
  fn from(v: Vec2) -> Self {
    [v.x, v.y]
  }
}

impl Add for Vec2 {
  type Output = Vec2;
  fn add(self, o: Vec2) -> Vec2 {
    Vec2::new(self.x + o.x, self.y + o.y)
  }
}
impl Sub for Vec2 {
  type Output = Vec2;
  fn sub(self, o: Vec2) -> Vec2 {
    Vec2::new(self.x - o.x, self.y - o.y)
  }
}
impl Mul<f32> for Vec2 {
  type Output = Vec2;
  fn mul(self, s: f32) -> Vec2 {
    Vec2::new(self.x * s, self.y * s)
  }
}

/// Serialized as `[x, y, z]`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "[f32; 3]", into = "[f32; 3]")]
pub struct Vec3 {
  pub x: f32,
  pub y: f32,
  pub z: f32,
}

impl Vec3 {
  pub const fn new(x: f32, y: f32, z: f32) -> Self {
    Self { x, y, z }
  }
}

impl From<[f32; 3]> for Vec3 {
  fn from(a: [f32; 3]) -> Self {
    Self::new(a[0], a[1], a[2])
  }
}
impl From<Vec3> for [f32; 3] {
  fn from(v: Vec3) -> Self {
    [v.x, v.y, v.z]
  }
}

impl Add for Vec3 {
  type Output = Vec3;
  fn add(self, o: Vec3) -> Vec3 {
    Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
  }
}
impl Sub for Vec3 {
  type Output = Vec3;
  fn sub(self, o: Vec3) -> Vec3 {
    Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
  }
}
impl Mul<f32> for Vec3 {
  type Output = Vec3;
  fn mul(self, s: f32) -> Vec3 {
    Vec3::new(self.x * s, self.y * s, self.z * s)
  }
}
impl Mul<Vec3> for f32 {
  type Output = Vec3;
  fn mul(self, v: Vec3) -> Vec3 {
    v * self
  }
}
impl Div<f32> for Vec3 {
  type Output = Vec3;
  fn div(self, s: f32) -> Vec3 {
    Vec3::new(self.x / s, self.y / s, self.z / s)
  }
}
impl Neg for Vec3 {
  type Output = Vec3;
  fn neg(self) -> Vec3 {
    Vec3::new(-self.x, -self.y, -self.z)
  }
}
impl AddAssign for Vec3 {
  fn add_assign(&mut self, o: Vec3) {
    self.x += o.x;
    self.y += o.y;
    self.z += o.z;
  }
}
impl SubAssign for Vec3 {
  fn sub_assign(&mut self, o: Vec3) {
    self.x -= o.x;
    self.y -= o.y;
    self.z -= o.z;
  }
}
impl MulAssign<f32> for Vec3 {
  fn mul_assign(&mut self, s: f32) {
    self.x *= s;
    self.y *= s;
    self.z *= s;
  }
}
impl Index<usize> for Vec3 {
  type Output = f32;
  fn index(&self, i: usize) -> &f32 {
    match i {
      0 => &self.x,
      1 => &self.y,
      2 => &self.z,
      _ => panic!("Vec3 index {i}"),
    }
  }
}
impl IndexMut<usize> for Vec3 {
  fn index_mut(&mut self, i: usize) -> &mut f32 {
    match i {
      0 => &mut self.x,
      1 => &mut self.y,
      2 => &mut self.z,
      _ => panic!("Vec3 index {i}"),
    }
  }
}

/// Component-wise product.
pub fn mul(a: Vec3, b: Vec3) -> Vec3 {
  Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}
pub fn dot(a: Vec3, b: Vec3) -> f32 {
  a.x * b.x + a.y * b.y + a.z * b.z
}
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
  Vec3::new(
    a.y * b.z - a.z * b.y,
    a.z * b.x - a.x * b.z,
    a.x * b.y - a.y * b.x,
  )
}
pub fn length2(v: Vec3) -> f32 {
  dot(v, v)
}
pub fn length(v: Vec3) -> f32 {
  dot(v, v).sqrt()
}
pub fn normalize(v: Vec3) -> Vec3 {
  let l = length(v);
  if l > 1e-12 {
    v / l
  } else {
    Vec3::new(0.0, 0.0, 0.0)
  }
}
pub fn vmin(a: Vec3, b: Vec3) -> Vec3 {
  Vec3::new(fmin(a.x, b.x), fmin(a.y, b.y), fmin(a.z, b.z))
}
pub fn vmax(a: Vec3, b: Vec3) -> Vec3 {
  Vec3::new(fmax(a.x, b.x), fmax(a.y, b.y), fmax(a.z, b.z))
}
pub fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
  a + (b - a) * t
}

/// Serialized as `[x, y, z, w]`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "[f32; 4]", into = "[f32; 4]")]
pub struct Vec4 {
  pub x: f32,
  pub y: f32,
  pub z: f32,
  pub w: f32,
}

impl Vec4 {
  pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
    Self { x, y, z, w }
  }
  pub const fn from_vec3(v: Vec3, w: f32) -> Self {
    Self::new(v.x, v.y, v.z, w)
  }
  pub fn xyz(&self) -> Vec3 {
    Vec3::new(self.x, self.y, self.z)
  }
}

impl From<[f32; 4]> for Vec4 {
  fn from(a: [f32; 4]) -> Self {
    Self::new(a[0], a[1], a[2], a[3])
  }
}
impl From<Vec4> for [f32; 4] {
  fn from(v: Vec4) -> Self {
    [v.x, v.y, v.z, v.w]
  }
}
impl Index<usize> for Vec4 {
  type Output = f32;
  fn index(&self, i: usize) -> &f32 {
    match i {
      0 => &self.x,
      1 => &self.y,
      2 => &self.z,
      3 => &self.w,
      _ => panic!("Vec4 index {i}"),
    }
  }
}
impl IndexMut<usize> for Vec4 {
  fn index_mut(&mut self, i: usize) -> &mut f32 {
    match i {
      0 => &mut self.x,
      1 => &mut self.y,
      2 => &mut self.z,
      3 => &mut self.w,
      _ => panic!("Vec4 index {i}"),
    }
  }
}
impl Add for Vec4 {
  type Output = Vec4;
  fn add(self, o: Vec4) -> Vec4 {
    Vec4::new(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
  }
}
impl Mul<f32> for Vec4 {
  type Output = Vec4;
  fn mul(self, s: f32) -> Vec4 {
    Vec4::new(self.x * s, self.y * s, self.z * s, self.w * s)
  }
}

// ------------------------------------------------------------------------------------------------
// quaternions
// ------------------------------------------------------------------------------------------------

/// Serialized as `[x, y, z, w]` (glTF's order). The default is the identity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f32; 4]", into = "[f32; 4]")]
pub struct Quat {
  pub x: f32,
  pub y: f32,
  pub z: f32,
  pub w: f32,
}

impl Default for Quat {
  fn default() -> Self {
    Self::new(0.0, 0.0, 0.0, 1.0)
  }
}

impl From<[f32; 4]> for Quat {
  fn from(a: [f32; 4]) -> Self {
    Self::new(a[0], a[1], a[2], a[3])
  }
}
impl From<Quat> for [f32; 4] {
  fn from(q: Quat) -> Self {
    [q.x, q.y, q.z, q.w]
  }
}

impl Quat {
  pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
    Self { x, y, z, w }
  }

  pub fn axis_angle(axis: Vec3, rad: f32) -> Quat {
    let a = normalize(axis);
    let s = (rad * 0.5).sin();
    Quat::new(a.x * s, a.y * s, a.z * s, (rad * 0.5).cos())
  }

  pub fn conj(&self) -> Quat {
    Quat::new(-self.x, -self.y, -self.z, self.w)
  }

  pub fn normalized(&self) -> Quat {
    let l = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
    if l < 1e-12 {
      return Quat::default();
    }
    Quat::new(self.x / l, self.y / l, self.z / l, self.w / l)
  }

  pub fn rotate(&self, v: Vec3) -> Vec3 {
    let u = Vec3::new(self.x, self.y, self.z);
    let t = cross(u, v) * 2.0;
    v + t * self.w + cross(u, t)
  }

  /// Tait-Bryan angles in degrees, q = qz * qy * qx (X applied first).
  pub fn from_euler_deg(deg: Vec3) -> Quat {
    let qx = Quat::axis_angle(Vec3::new(1.0, 0.0, 0.0), radians(deg.x));
    let qy = Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(deg.y));
    let qz = Quat::axis_angle(Vec3::new(0.0, 0.0, 1.0), radians(deg.z));
    (qz * qy * qx).normalized()
  }

  pub fn to_euler_deg(&self) -> Vec3 {
    // from the rotation matrix, in double: at a quarter turn around y (gimbal lock) x and z turn
    // around the same axis, so z is 0 and x takes the whole turn
    let (qx, qy, qz, qw) = (self.x as f64, self.y as f64, self.z as f64, self.w as f64);
    let r20 = 2.0 * (qx * qz - qw * qy);
    let ry = (-r20).clamp(-1.0, 1.0).asin();
    let (rx, rz);
    if r20.abs() < 1.0 - 1e-6 {
      rx = (2.0 * (qy * qz + qw * qx)).atan2(1.0 - 2.0 * (qx * qx + qy * qy));
      rz = (2.0 * (qx * qy + qw * qz)).atan2(1.0 - 2.0 * (qy * qy + qz * qz));
    } else {
      rx = (-2.0 * (qy * qz - qw * qx)).atan2(1.0 - 2.0 * (qx * qx + qz * qz));
      rz = 0.0;
    }
    Vec3::new(degrees(rx as f32), degrees(ry as f32), degrees(rz as f32))
  }

  /// The rotation part, columns must be orthonormal.
  pub fn from_matrix(a: &Mat4) -> Quat {
    let (m00, m11, m22) = (a.at(0, 0), a.at(1, 1), a.at(2, 2));
    let tr = m00 + m11 + m22;
    let mut q = Quat::default();
    if tr > 0.0 {
      let s = (tr + 1.0).sqrt() * 2.0;
      q.w = 0.25 * s;
      q.x = (a.at(2, 1) - a.at(1, 2)) / s;
      q.y = (a.at(0, 2) - a.at(2, 0)) / s;
      q.z = (a.at(1, 0) - a.at(0, 1)) / s;
    } else if m00 > m11 && m00 > m22 {
      let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
      q.w = (a.at(2, 1) - a.at(1, 2)) / s;
      q.x = 0.25 * s;
      q.y = (a.at(0, 1) + a.at(1, 0)) / s;
      q.z = (a.at(0, 2) + a.at(2, 0)) / s;
    } else if m11 > m22 {
      let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
      q.w = (a.at(0, 2) - a.at(2, 0)) / s;
      q.x = (a.at(0, 1) + a.at(1, 0)) / s;
      q.y = 0.25 * s;
      q.z = (a.at(1, 2) + a.at(2, 1)) / s;
    } else {
      let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
      q.w = (a.at(1, 0) - a.at(0, 1)) / s;
      q.x = (a.at(0, 2) + a.at(2, 0)) / s;
      q.y = (a.at(1, 2) + a.at(2, 1)) / s;
      q.z = 0.25 * s;
    }
    q.normalized()
  }
}

impl Mul for Quat {
  type Output = Quat;
  fn mul(self, b: Quat) -> Quat {
    let (x, y, z, w) = (self.x, self.y, self.z, self.w);
    Quat::new(
      w * b.x + x * b.w + y * b.z - z * b.y,
      w * b.y - x * b.z + y * b.w + z * b.x,
      w * b.z + x * b.y - y * b.x + z * b.w,
      w * b.w - x * b.x - y * b.y - z * b.z,
    )
  }
}

// ------------------------------------------------------------------------------------------------
// matrices
// ------------------------------------------------------------------------------------------------

/// Column-major: `m[col * 4 + row]`. The default is the identity. Serialized as its 16 floats.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f32; 16]", into = "[f32; 16]")]
pub struct Mat4 {
  pub m: [f32; 16],
}

impl Default for Mat4 {
  fn default() -> Self {
    Self::identity()
  }
}

impl From<[f32; 16]> for Mat4 {
  fn from(m: [f32; 16]) -> Self {
    Self { m }
  }
}
impl From<Mat4> for [f32; 16] {
  fn from(m: Mat4) -> Self {
    m.m
  }
}

impl Mat4 {
  pub const fn identity() -> Mat4 {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    Mat4 { m }
  }
  /// Every element `fill` (C++ `Mat4(float fill)`).
  pub const fn fill(fill: f32) -> Mat4 {
    Mat4 { m: [fill; 16] }
  }
  pub fn at(&self, row: usize, col: usize) -> f32 {
    self.m[col * 4 + row]
  }
  pub fn at_mut(&mut self, row: usize, col: usize) -> &mut f32 {
    &mut self.m[col * 4 + row]
  }
  pub fn col3(&self, c: usize) -> Vec3 {
    Vec3::new(self.m[c * 4], self.m[c * 4 + 1], self.m[c * 4 + 2])
  }
  pub fn set_col3(&mut self, c: usize, v: Vec3) {
    self.m[c * 4] = v.x;
    self.m[c * 4 + 1] = v.y;
    self.m[c * 4 + 2] = v.z;
  }

  pub fn translate(t: Vec3) -> Mat4 {
    let mut r = Mat4::identity();
    r.m[12] = t.x;
    r.m[13] = t.y;
    r.m[14] = t.z;
    r
  }
  pub fn scale(s: Vec3) -> Mat4 {
    let mut r = Mat4::identity();
    r.m[0] = s.x;
    r.m[5] = s.y;
    r.m[10] = s.z;
    r
  }
  pub fn rotate(q: Quat) -> Mat4 {
    let mut r = Mat4::identity();
    let (xx, yy, zz) = (q.x * q.x, q.y * q.y, q.z * q.z);
    let (xy, xz, yz) = (q.x * q.y, q.x * q.z, q.y * q.z);
    let (wx, wy, wz) = (q.w * q.x, q.w * q.y, q.w * q.z);
    r.m[0] = 1.0 - 2.0 * (yy + zz);
    r.m[1] = 2.0 * (xy + wz);
    r.m[2] = 2.0 * (xz - wy);
    r.m[4] = 2.0 * (xy - wz);
    r.m[5] = 1.0 - 2.0 * (xx + zz);
    r.m[6] = 2.0 * (yz + wx);
    r.m[8] = 2.0 * (xz + wy);
    r.m[9] = 2.0 * (yz - wx);
    r.m[10] = 1.0 - 2.0 * (xx + yy);
    r
  }
  pub fn perspective(fovy_rad: f32, aspect: f32, znear: f32, zfar: f32) -> Mat4 {
    let mut r = Mat4::fill(0.0);
    let f = 1.0 / (fovy_rad * 0.5).tan();
    r.m[0] = f / aspect;
    r.m[5] = f;
    r.m[10] = (zfar + znear) / (znear - zfar);
    r.m[11] = -1.0;
    r.m[14] = 2.0 * zfar * znear / (znear - zfar);
    r
  }
  pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = normalize(target - eye);
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    let mut r = Mat4::identity();
    r.m[0] = s.x;
    r.m[4] = s.y;
    r.m[8] = s.z;
    r.m[1] = u.x;
    r.m[5] = u.y;
    r.m[9] = u.z;
    r.m[2] = -f.x;
    r.m[6] = -f.y;
    r.m[10] = -f.z;
    r.m[12] = -dot(s, eye);
    r.m[13] = -dot(u, eye);
    r.m[14] = dot(f, eye);
    r
  }

  pub fn point(&self, p: Vec3) -> Vec3 {
    let m = &self.m;
    Vec3::new(
      m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
      m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
      m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
    )
  }
  pub fn dir(&self, d: Vec3) -> Vec3 {
    let m = &self.m;
    Vec3::new(
      m[0] * d.x + m[4] * d.y + m[8] * d.z,
      m[1] * d.x + m[5] * d.y + m[9] * d.z,
      m[2] * d.x + m[6] * d.y + m[10] * d.z,
    )
  }
  pub fn transposed(&self) -> Mat4 {
    let mut r = Mat4::fill(0.0);
    for c in 0..4 {
      for rr in 0..4 {
        *r.at_mut(rr, c) = self.at(c, rr);
      }
    }
    r
  }
  pub fn det3(&self) -> f32 {
    self.at(0, 0) * (self.at(1, 1) * self.at(2, 2) - self.at(1, 2) * self.at(2, 1))
      - self.at(0, 1) * (self.at(1, 0) * self.at(2, 2) - self.at(1, 2) * self.at(2, 0))
      + self.at(0, 2) * (self.at(1, 0) * self.at(2, 1) - self.at(1, 1) * self.at(2, 0))
  }

  /// The identity when the matrix cannot be inverted.
  pub fn inverse(&self) -> Mat4 {
    let a = &self.m;
    let mut inv = [0.0f32; 16];
    inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15]
      + a[9] * a[7] * a[14]
      + a[13] * a[6] * a[11]
      - a[13] * a[7] * a[10];
    inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15]
      - a[8] * a[7] * a[14]
      - a[12] * a[6] * a[11]
      + a[12] * a[7] * a[10];
    inv[8] = a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15]
      + a[8] * a[7] * a[13]
      + a[12] * a[5] * a[11]
      - a[12] * a[7] * a[9];
    inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14]
      - a[8] * a[6] * a[13]
      - a[12] * a[5] * a[10]
      + a[12] * a[6] * a[9];
    inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15]
      - a[9] * a[3] * a[14]
      - a[13] * a[2] * a[11]
      + a[13] * a[3] * a[10];
    inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15]
      + a[8] * a[3] * a[14]
      + a[12] * a[2] * a[11]
      - a[12] * a[3] * a[10];
    inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15]
      - a[8] * a[3] * a[13]
      - a[12] * a[1] * a[11]
      + a[12] * a[3] * a[9];
    inv[13] = a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14]
      + a[8] * a[2] * a[13]
      + a[12] * a[1] * a[10]
      - a[12] * a[2] * a[9];
    inv[2] = a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15]
      + a[5] * a[3] * a[14]
      + a[13] * a[2] * a[7]
      - a[13] * a[3] * a[6];
    inv[6] = -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15]
      - a[4] * a[3] * a[14]
      - a[12] * a[2] * a[7]
      + a[12] * a[3] * a[6];
    inv[10] = a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15]
      + a[4] * a[3] * a[13]
      + a[12] * a[1] * a[7]
      - a[12] * a[3] * a[5];
    inv[14] = -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14]
      - a[4] * a[2] * a[13]
      - a[12] * a[1] * a[6]
      + a[12] * a[2] * a[5];
    inv[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11]
      - a[5] * a[3] * a[10]
      - a[9] * a[2] * a[7]
      + a[9] * a[3] * a[6];
    inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11]
      + a[4] * a[3] * a[10]
      + a[8] * a[2] * a[7]
      - a[8] * a[3] * a[6];
    inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11]
      - a[4] * a[3] * a[9]
      - a[8] * a[1] * a[7]
      + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10]
      + a[4] * a[2] * a[9]
      + a[8] * a[1] * a[6]
      - a[8] * a[2] * a[5];
    let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if det.abs() < 1e-20 {
      return Mat4::identity();
    }
    let det = 1.0 / det;
    Mat4 {
      m: inv.map(|v| v * det),
    }
  }
}

impl Mul for Mat4 {
  type Output = Mat4;
  fn mul(self, b: Mat4) -> Mat4 {
    let mut r = Mat4::fill(0.0);
    for c in 0..4 {
      for rr in 0..4 {
        let mut s = 0.0;
        for k in 0..4 {
          s += self.at(rr, k) * b.at(k, c);
        }
        *r.at_mut(rr, c) = s;
      }
    }
    r
  }
}
impl Mul<Vec4> for Mat4 {
  type Output = Vec4;
  fn mul(self, v: Vec4) -> Vec4 {
    let m = &self.m;
    Vec4::new(
      m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12] * v.w,
      m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13] * v.w,
      m[2] * v.x + m[6] * v.y + m[10] * v.z + m[14] * v.w,
      m[3] * v.x + m[7] * v.y + m[11] * v.z + m[15] * v.w,
    )
  }
}

// ------------------------------------------------------------------------------------------------
// transforms, boxes, rays
// ------------------------------------------------------------------------------------------------

/// Translation, rotation, scale: matrix = T * R * S.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
  pub t: Vec3,
  pub r: Quat,
  pub s: Vec3,
}

impl Default for Transform {
  fn default() -> Self {
    Self {
      t: Vec3::new(0.0, 0.0, 0.0),
      r: Quat::default(),
      s: Vec3::new(1.0, 1.0, 1.0),
    }
  }
}

impl Transform {
  pub fn matrix(&self) -> Mat4 {
    Mat4::translate(self.t) * Mat4::rotate(self.r) * Mat4::scale(self.s)
  }
  pub fn from_matrix(m: &Mat4) -> Transform {
    let mut out = Transform {
      t: m.col3(3),
      ..Default::default()
    };
    let (c0, c1, c2) = (m.col3(0), m.col3(1), m.col3(2));
    out.s = Vec3::new(length(c0), length(c1), length(c2));
    if m.det3() < 0.0 {
      out.s.x = -out.s.x;
    }
    let mut rot = Mat4::identity();
    rot.set_col3(
      0,
      if out.s.x != 0.0 {
        c0 / out.s.x
      } else {
        Vec3::new(1.0, 0.0, 0.0)
      },
    );
    rot.set_col3(
      1,
      if out.s.y != 0.0 {
        c1 / out.s.y
      } else {
        Vec3::new(0.0, 1.0, 0.0)
      },
    );
    rot.set_col3(
      2,
      if out.s.z != 0.0 {
        c2 / out.s.z
      } else {
        Vec3::new(0.0, 0.0, 1.0)
      },
    );
    out.r = Quat::from_matrix(&rot);
    out
  }
}

/// The default box is empty (not `valid()`): adding points grows it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AABB {
  pub lo: Vec3,
  pub hi: Vec3,
}

impl Default for AABB {
  fn default() -> Self {
    Self {
      lo: Vec3::new(f32::MAX, f32::MAX, f32::MAX),
      hi: Vec3::new(-f32::MAX, -f32::MAX, -f32::MAX),
    }
  }
}

impl AABB {
  pub fn valid(&self) -> bool {
    self.lo.x <= self.hi.x && self.lo.y <= self.hi.y && self.lo.z <= self.hi.z
  }
  pub fn add(&mut self, p: Vec3) {
    self.lo = vmin(self.lo, p);
    self.hi = vmax(self.hi, p);
  }
  pub fn add_box(&mut self, b: &AABB) {
    if !b.valid() {
      return;
    }
    self.lo = vmin(self.lo, b.lo);
    self.hi = vmax(self.hi, b.hi);
  }
  pub fn center(&self) -> Vec3 {
    (self.lo + self.hi) * 0.5
  }
  pub fn size(&self) -> Vec3 {
    self.hi - self.lo
  }
  pub fn radius(&self) -> f32 {
    length(self.hi - self.lo) * 0.5
  }
  pub fn expanded(&self, e: f32) -> AABB {
    let mut r = *self;
    r.lo -= Vec3::new(e, e, e);
    r.hi += Vec3::new(e, e, e);
    r
  }
  pub fn overlaps(&self, o: &AABB) -> bool {
    self.lo.x <= o.hi.x
      && self.hi.x >= o.lo.x
      && self.lo.y <= o.hi.y
      && self.hi.y >= o.lo.y
      && self.lo.z <= o.hi.z
      && self.hi.z >= o.lo.z
  }
  pub fn contains(&self, p: Vec3) -> bool {
    p.x >= self.lo.x
      && p.x <= self.hi.x
      && p.y >= self.lo.y
      && p.y <= self.hi.y
      && p.z >= self.lo.z
      && p.z <= self.hi.z
  }
  /// the box around the transformed box (center and half extents: one point transform)
  pub fn transformed(&self, m: &Mat4) -> AABB {
    let mut r = AABB::default();
    if !self.valid() {
      return r;
    }
    let c = (self.lo + self.hi) * 0.5;
    let e = (self.hi - self.lo) * 0.5;
    let nc = m.point(c);
    let ne = Vec3::new(
      m.at(0, 0).abs() * e.x + m.at(0, 1).abs() * e.y + m.at(0, 2).abs() * e.z,
      m.at(1, 0).abs() * e.x + m.at(1, 1).abs() * e.y + m.at(1, 2).abs() * e.z,
      m.at(2, 0).abs() * e.x + m.at(2, 1).abs() * e.y + m.at(2, 2).abs() * e.z,
    );
    r.lo = nc - ne;
    r.hi = nc + ne;
    r
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ray {
  pub o: Vec3,
  /// not necessarily normalized when transformed into a local space
  pub d: Vec3,
}

impl Ray {
  pub const fn new(o: Vec3, d: Vec3) -> Self {
    Self { o, d }
  }
  pub fn at(&self, t: f32) -> Vec3 {
    self.o + self.d * t
  }
}

pub fn transform_ray(m: &Mat4, r: &Ray) -> Ray {
  Ray::new(m.point(r.o), m.dir(r.d))
}

/// Slab test. Returns the distance where the ray enters the box when it hits it within [0, tmax].
pub fn ray_aabb(r: &Ray, b: &AABB, tmax: f32) -> Option<f32> {
  let (mut t0, mut t1) = (0.0f32, tmax);
  for a in 0..3 {
    let inv = 1.0 / r.d[a];
    let mut tn = (b.lo[a] - r.o[a]) * inv;
    let mut tf = (b.hi[a] - r.o[a]) * inv;
    if tn > tf {
      std::mem::swap(&mut tn, &mut tf);
    }
    t0 = if tn > t0 { tn } else { t0 };
    t1 = if tf < t1 { tf } else { t1 };
    if t0 > t1 {
      return None;
    }
  }
  Some(t0)
}

/// Moller-Trumbore. Returns the distance along the ray, or -1 when there is no hit.
pub fn ray_triangle(r: &Ray, a: Vec3, b: Vec3, c: Vec3) -> f32 {
  let e1 = b - a;
  let e2 = c - a;
  let p = cross(r.d, e2);
  let det = dot(e1, p);
  if det.abs() < 1e-12 {
    return -1.0;
  }
  let inv = 1.0 / det;
  let s = r.o - a;
  let u = dot(s, p) * inv;
  // the C++ comparisons (not a range): a NaN goes on
  if u < 0.0 || u > 1.0 {
    return -1.0;
  }
  let q = cross(s, e1);
  let v = dot(r.d, q) * inv;
  if v < 0.0 || u + v > 1.0 {
    return -1.0;
  }
  let t = dot(e2, q) * inv;
  if t > 0.0 { t } else { -1.0 }
}

/// The distance to the plane through `p` of normal `n`, None when the ray runs along it.
pub fn ray_plane(r: &Ray, p: Vec3, n: Vec3) -> Option<f32> {
  let den = dot(n, r.d);
  if den.abs() < 1e-9 {
    return None;
  }
  Some(dot(p - r.o, n) / den)
}

/// The distance to the sphere, or -1 when the ray misses it.
pub fn ray_sphere(r: &Ray, c: Vec3, radius: f32) -> f32 {
  let oc = r.o - c;
  let a = dot(r.d, r.d);
  let b = dot(oc, r.d);
  let cc = dot(oc, oc) - radius * radius;
  let disc = b * b - a * cc;
  if disc < 0.0 {
    return -1.0;
  }
  let s = disc.sqrt();
  let mut t = (-b - s) / a;
  if t < 0.0 {
    t = (-b + s) / a;
  }
  if t >= 0.0 { t } else { -1.0 }
}

/// RGBA 0..1 as the bytes of a u32, red in the low byte.
pub fn pack_rgba(r: f32, g: f32, b: f32, a: f32) -> u32 {
  let c = |v: f32| ((v * 255.0).round() as i32).clamp(0, 255) as u32;
  c(r) | (c(g) << 8) | (c(b) << 16) | (c(a) << 24)
}

#[cfg(test)]
mod tests {
  use super::*;

  // the selftest's `// math` block
  #[test]
  fn euler_quaternion_round_trip() {
    let q = Quat::from_euler_deg(Vec3::new(10.0, 35.0, -20.0));
    let e = q.to_euler_deg();
    assert!(
      (e.x - 10.0).abs() < 0.01 && (e.y - 35.0).abs() < 0.01 && (e.z + 20.0).abs() < 0.01,
      "euler <-> quaternion"
    );
    // the angles read back give the same rotation, also turned a quarter around y and tilted (a
    // ramp turned 90 degrees): what the inspector writes back when a field is edited
    let axes = [
      Vec3::new(1.0, 0.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
      Vec3::new(0.0, 0.0, 1.0),
    ];
    for yaw in [90.0f32, -90.0, 89.99] {
      for tilt in [
        Quat::axis_angle(Vec3::new(1.0, 0.0, 0.0), 0.4),
        Quat::axis_angle(Vec3::new(0.0, 0.0, 1.0), -0.3),
        Quat::axis_angle(normalize(Vec3::new(1.0, 0.0, 1.0)), 0.5),
      ] {
        let r = (Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), radians(yaw)) * tilt).normalized();
        let back = Quat::from_euler_deg(r.to_euler_deg());
        for a in axes {
          assert!(
            length(back.rotate(a) - r.rotate(a)) < 1e-3,
            "euler: a quarter turn around y with a tilt"
          );
        }
      }
    }
    let t = Transform {
      t: Vec3::new(1.0, 2.0, 3.0),
      r: q,
      s: Vec3::new(2.0, 3.0, 4.0),
    };
    let back = Transform::from_matrix(&t.matrix());
    assert!(
      length(back.t - t.t) < 1e-4
        && length(back.s - t.s) < 1e-4
        && (back.r.w.abs() - q.w.abs()).abs() < 1e-4,
      "TRS -> matrix -> TRS"
    );
    let m = t.matrix();
    let id = m * m.inverse();
    assert!(
      (id.at(0, 0) - 1.0).abs() < 1e-4 && id.at(1, 3).abs() < 1e-4,
      "matrix inverse"
    );
  }

  #[test]
  fn rays() {
    let b = AABB {
      lo: Vec3::new(-1.0, -1.0, -1.0),
      hi: Vec3::new(1.0, 1.0, 1.0),
    };
    let r = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
    assert_eq!(ray_aabb(&r, &b, f32::MAX), Some(4.0));
    assert_eq!(ray_aabb(&r, &b, 3.0), None, "the box beyond tmax");
    // a ray along an axis (division by zero in the slab test) inside the slab hits, outside misses
    assert!(
      ray_aabb(
        &Ray::new(Vec3::new(0.5, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        &b,
        100.0
      )
      .is_some()
    );
    assert!(
      ray_aabb(
        &Ray::new(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        &b,
        100.0
      )
      .is_none()
    );
    let t = ray_triangle(
      &r,
      Vec3::new(-1.0, -1.0, 0.0),
      Vec3::new(1.0, -1.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
    );
    assert!((t - 5.0).abs() < 1e-6);
    assert_eq!(
      ray_triangle(
        &r,
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(3.0, 2.0, 0.0),
        Vec3::new(2.0, 3.0, 0.0)
      ),
      -1.0
    );
    assert!((ray_sphere(&r, Vec3::default(), 1.0) - 4.0).abs() < 1e-6);
    assert_eq!(
      ray_plane(&r, Vec3::default(), Vec3::new(0.0, 0.0, 1.0)),
      Some(5.0)
    );
    // a rotated box's bounds
    let m = Mat4::rotate(Quat::axis_angle(Vec3::new(0.0, 1.0, 0.0), K_PI / 4.0));
    let tb = b.transformed(&m);
    assert!((tb.hi.x - 2.0f32.sqrt()).abs() < 1e-5 && (tb.hi.y - 1.0).abs() < 1e-5);
    assert!(!AABB::default().valid());
    assert_eq!(pack_rgba(1.0, 0.0, 0.5, 1.0), 0xff80_00ff);
    assert_eq!(
      serde_json::to_string(&Vec3::new(1.0, 2.0, 3.0)).unwrap(),
      "[1.0,2.0,3.0]"
    );
  }
}

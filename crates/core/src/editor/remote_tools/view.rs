//! The MCP server's tools on the view: what it shows, the camera, a picture of it.

use super::*;

impl Editor {
  pub(super) fn tool_set_view(&mut self, a: &Value) -> R {
    // the game view (the game's renderer), its hour and fog, the effects
    if let Some(on) = a.get("game_view").and_then(Value::as_bool) {
      if on && !self.view.game_view_ready {
        return Err(format!("no game view: {}", self.view.game_view_error));
      }
      self.view.game_view = on;
    }
    if let Some(h) = num_opt(a, "hour") {
      self.weather.mood.hour = h.clamp(0.0, 24.0);
    }
    // the weather: one of the game's by its name, or its cloud, fog and rain
    if let Some(w) = a.get("weather").and_then(Value::as_str)
      && !self.weather.set_weather(w)
    {
      let names: Vec<&str> = weather_presets().iter().map(|p| p.id).collect();
      return Err(format!("weather: one of {}", names.join(", ")));
    }
    let mood = &mut self.weather.mood;
    for (k, v) in [
      ("cloud", &mut mood.cloud),
      ("fog", &mut mood.fog),
      ("rain", &mut mood.rain),
    ] {
      if let Some(x) = num_opt(a, k) {
        *v = x.clamp(0.0, 1.0);
      }
    }
    if let Some(l) = a.get("level_lights").and_then(Value::as_bool) {
      mood.level_lights = l;
    }
    if let Some(l) = a.get("lightning").and_then(Value::as_bool) {
      self.weather.lightning = l;
    }
    if let Some(e) = a.get("effects").and_then(Value::as_bool) {
      self.view.effects = e;
    }
    Ok(self.view_json())
  }

  pub(super) fn tool_set_camera(&mut self, a: &Value) -> R {
    // the camera computed here (render/camera.h), then given to the viewport
    let mut cam = self.camera;
    let mut framed = AABB::default();
    if let Some(f) = a.get("frame") {
      let roles = [LevelRole::Main, LevelRole::Added, LevelRole::Companion];
      if f.as_str() == Some("all") {
        // app.cpp frame_all
        cam.yaw = 0.6;
        cam.pitch = -0.45;
        framed = self.scene.focus_bounds();
      } else if let Some(level) = f.get("level") {
        let g = self
          .document_level_group(level, &roles)
          .ok_or("frame.level: a level of the document (get_status: document.levels)")?;
        framed = self.document_level_bounds(g);
        if !framed.valid() {
          return Err("frame.level: that level has nothing to frame".into());
        }
      } else if let Some(pair) = f.get("junction") {
        let group = |k: usize| {
          pair
            .as_array()
            .filter(|p| p.len() == 2)
            .and_then(|p| self.document_level_group(&p[k], &roles))
        };
        let (Some(ga), Some(gb)) = (group(0), group(1)) else {
          return Err("frame.junction: two levels of the document".into());
        };
        if ga == gb {
          return Err("frame.junction: two levels of the document".into());
        }
        framed = self.document_junction(ga, gb);
        if !framed.valid() {
          return Err("frame.junction: these two levels do not meet".into());
        }
        cam.pitch = -0.6;
      } else {
        let ids: Vec<u32> = if f.is_array() {
          ids_arg(a, "frame")
        } else {
          self.selection.iter().copied().collect()
        };
        for id in ids {
          framed.add_box(&self.subtree_bounds(id).0);
        }
        if framed.valid() {
          framed = framed.expanded(2.0);
        }
      }
      cam.frame(framed);
    }
    if let Some(p) = vec3_arg(a, "position") {
      cam.pos = p;
    }
    if let Some(t) = vec3_arg(a, "look_at") {
      cam.look_at(t);
    }
    if let Some(y) = num_opt(a, "yaw_deg") {
      cam.yaw = radians(y);
    }
    if let Some(p) = num_opt(a, "pitch_deg") {
      cam.pitch = radians(p).clamp(-1.55, 1.55);
    }
    self.camera = cam;
    let mut request = json!({"kind": "camera", "pos": cam.pos, "yaw": cam.yaw, "pitch": cam.pitch});
    if framed.valid() {
      // the viewport's speed for a box that size (the position given after it)
      request["frame"] = json!({"lo": framed.lo, "hi": framed.hi});
    }
    self.request(request);
    Ok(camera_json(&cam))
  }

  pub(super) fn tool_capture_view(&mut self, a: &Value) -> Reply {
    // the picture is the interface's: asked once it listens (frontend_ready), a few seconds after
    // the editor started
    if !self.interface_ready {
      let (a, start) = (a.clone(), Instant::now());
      return Reply::wait(move |ed| {
        if ed.interface_ready {
          return Some(ed.tool_capture_view(&a));
        }
        (start.elapsed() > K_INTERFACE_WAIT).then(|| {
          Err(
            "the view could not be captured: the editor's interface is not ready (its window is              still starting, or there is none)"
              .to_string(),
          )
          .into()
        })
      });
    }
    let w = int(a, "width", 960).clamp(64, 2048);
    let h = int(a, "height", 600).clamp(64, 2048);
    // what the user's view shows by default: the game rendering at its hour when it is on
    let shown = if self.view.game_view && self.view.game_view_ready {
      self.weather.mood.hour
    } else {
      -1.0
    };
    let hour = num(a, "game_hour", shown);
    if hour >= 0.0 && !self.view.game_view_ready {
      return Err(format!("no game view: {}", self.view.game_view_error)).into();
    }
    // the picture is the viewport's: {kind: "capture", width, height, game_hour (-1: the
    // editor's own view), only: these elements and what is under them alone}
    let mut request = json!({"kind": "capture", "width": w, "height": h, "game_hour": hour});
    if a.get("only").is_some() {
      let mut keep = BTreeSet::new();
      for id in ids_arg(a, "only") {
        keep.insert(id);
        keep.extend(self.scene.descendants(id));
      }
      request["only"] = json!(keep);
    }
    let id = self.request(request);
    let start = Instant::now();
    Reply::wait(move |ed| {
      let Some(r) = ed.replies.remove(&id) else {
        return (start.elapsed() > K_CAPTURE_WAIT).then(|| {
          Err("the view could not be captured (the editor's view did not answer)".to_string())
            .into()
        });
      };
      let png = str_arg(&r, "png");
      if png.is_empty() {
        let why = str_arg(&r, "error");
        return Some(
          Err(if why.is_empty() {
            "the view could not be captured".to_string()
          } else {
            format!("the view could not be captured: {why}")
          })
          .into(),
        );
      }
      if let Some(c) = r
        .get("camera")
        .and_then(|c| serde_json::from_value::<CameraState>(c.clone()).ok())
      {
        ed.camera = c;
      }
      Some(Reply::Now(json!({
        "ok": true,
        "result": {"width": w, "height": h, "camera": camera_json(&ed.camera)},
        "image": png,
      })))
    })
  }
}

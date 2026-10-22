//! The MCP server's tools on levels and documents: list, open, add, remove, merge, replace a
//! level, save the project, its story.

use super::*;

impl Editor {
  pub(super) fn tool_list_levels(&mut self, a: &Value) -> R {
    let f = lower(str_arg(a, "filter"));
    // near: only the levels the game displays together with that one
    let near = str_arg(a, "near");
    let around: Option<Vec<String>> = if near.is_empty() {
      None
    } else {
      let data = self.game_data();
      let n = data
        .as_ref()
        .and_then(|d| d.level(near))
        .map(|l| l.neighbors.clone());
      if n.is_none() {
        return Err(format!("near: no game data for the level '{near}'"));
      }
      n
    };
    let in_document: HashSet<String> = self
      .document_levels()
      .iter()
      .filter(|l| l.role != LevelRole::Layer)
      .map(|l| l.name.clone())
      .collect();
    let jak2 = str_arg(a, "game") == "jak2";
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    let levels: Vec<Value> = lib
      .levels()
      .iter()
      .filter(|l| l.listed && (f.is_empty() || l.name.contains(&f) || lower(&l.dgo).contains(&f)))
      .filter(|l| around.as_ref().is_none_or(|n| n.contains(&l.name)))
      .map(|l| {
        let mut e = json!({"name": l.name, "dgo": l.dgo});
        let name = if jak2 {
          format!("jak2-{}", l.name)
        } else {
          l.name.clone()
        };
        if in_document.contains(&name) {
          e["in_document"] = json!(true);
        }
        e
      })
      .collect();
    Ok(json!({ "levels": levels }))
  }

  pub(super) fn tool_open_level(&mut self, a: &Value) -> Reply {
    let name = str_arg(a, "name").to_string();
    if self.levels.opening.is_some() {
      return Err("a level is already loading".to_string()).into();
    }
    if self.remote_unsaved(a) {
      return Err(UNSAVED.to_string()).into();
    }
    // Jak II's (game "jak2"): its decor made Jak 3's, the document's level named jak2-<name>
    let jak2 = str_arg(a, "game") == "jak2";
    if jak2 && self.library_jak2.is_empty() {
      return Err(NO_JAK2.to_string()).into();
    }
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    if lib.level(&name).is_none() {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    }
    let before = self.load_actors;
    self.load_actors = flag(a, "actors", true);
    let started = self.load_level(&name, jak2);
    self.load_actors = before;
    if let Err(e) = started {
      return Err(e).into();
    }
    let opened = if jak2 {
      format!("jak2-{name}")
    } else {
      name.clone()
    };
    Reply::wait(move |ed| {
      ed.levels.opening.is_none().then(|| {
        if ed.scene.settings.source_group != opened {
          Err(format!(
            "the level {name} could not be opened (see the editor's log)"
          ))
          .into()
        } else {
          Ok(ed.remote_status()).into()
        }
      })
    })
  }

  pub(super) fn tool_add_level(&mut self, a: &Value) -> Reply {
    let name = str_arg(a, "name").to_string();
    let jak2 = str_arg(a, "game") == "jak2";
    let lib = if jak2 {
      &self.library_jak2
    } else {
      &self.library
    };
    if lib.level(&name).is_none() {
      return Err(format!(
        "no level '{name}' in the library (see list_levels)"
      ))
      .into();
    }
    let before = self.levels.levels_added;
    let with_actors = self.load_actors;
    self.load_actors = flag(a, "actors", true);
    let started = self.add_level_to_document(&name, flag(a, "reference", false), jak2);
    self.load_actors = with_actors;
    if let Err(e) = started {
      return Err(e).into();
    }
    // Jak II's is named jak2-<name> in the document
    let shown = if jak2 { format!("jak2-{name}") } else { name };
    Reply::wait(move |ed| {
      (ed.levels.levels_added != before).then(|| {
        if ed.levels.add_error.is_empty() {
          Ok(ed.level_info(&shown)).into()
        } else {
          Err(ed.levels.add_error.clone()).into()
        }
      })
    })
  }

  pub(super) fn tool_remove_level(&mut self, a: &Value) -> R {
    let name = str_arg(a, "name");
    let reference = flag(a, "reference", false);
    let group = self
      .document_levels()
      .iter()
      .rfind(|l| l.name == name && l.role == LevelRole::Added && l.reference == reference)
      .map(|l| l.group)
      .ok_or_else(|| format!("no level '{name}' added to the document (the level opened stays)"))?;
    self.remove_document_level(group)?;
    Ok(self.remote_status())
  }

  pub(super) fn tool_merge_level(&mut self, a: &Value) -> R {
    // a level added made part of the document's own: no longer a level next to it (the export
    // writes it with the document's, with its collision: a level built from another's pieces)
    let name = str_arg(a, "name");
    let group = self
      .document_levels()
      .iter()
      .rfind(|l| l.name == name && l.role == LevelRole::Added && !l.reference)
      .map(|l| l.group)
      .ok_or("name: a level added to the document (add_level), not a reference")?;
    let mut c = NodeChange::new(&self.scene);
    c.save(&self.scene, group);
    if let Some(o) = self
      .scene
      .find_mut(group)
      .and_then(|g| g.extras.as_object_mut())
    {
      o.remove("og_level");
    }
    self
      .history
      .push(c.commit(&self.scene, strf!(tr("undo.merge_level"), name), None));
    self.story_document_changed();
    Ok(json!({"group": group, "status": self.remote_status()}))
  }

  pub(super) fn tool_replace_level(&mut self, a: &Value) -> Reply {
    if !self.document_open() {
      return Err("no level open: open_level, open_project or new_level first".to_string()).into();
    }
    if let Some(level) = a.get("level") {
      let level = level.as_str().unwrap_or_default();
      let data = self.document_game_data();
      let info = data.as_ref().and_then(|d| d.level(level));
      if !level.is_empty() && info.is_none_or(|i| i.dgo.is_empty() || i.layer) {
        return Err(
          "level: a level of the game (see list_levels), or \"\" to replace none".to_string(),
        )
        .into();
      }
      self.set_replaces(level);
    }
    let level = self.scene.settings.replaces.clone();
    let shown = self
      .document_levels()
      .iter()
      .any(|l| l.reference && l.name == level);
    if flag(a, "show_original", false) && !level.is_empty() && !shown {
      if self.library.level(&level).is_none() {
        return Err(format!("the level {level} is not in the library")).into();
      }
      let before = self.levels.levels_added;
      if let Err(e) = self.add_level_to_document(&level, true, false) {
        return Err(e).into();
      }
      // what is asked once the original is there: placed where it is, turned
      let a = a.clone();
      return Reply::wait(move |ed| {
        (ed.levels.levels_added != before).then(|| {
          if ed.levels.add_error.is_empty() {
            ed.replace_level_finish(&a, &level).into()
          } else {
            Err(ed.levels.add_error.clone()).into()
          }
        })
      });
    }
    self.replace_level_finish(a, &level).into()
  }

  pub(super) fn replace_level_finish(&mut self, a: &Value, level: &str) -> R {
    if flag(a, "align", false) {
      self.align_to_replaced()?;
    }
    if let Some(t) = num_opt(a, "turn_deg") {
      self.turn_own_level(t);
    }
    self.scene.begin_frame();
    let data = self.document_game_data();
    let checks: Vec<Value> = check_continues(&self.scene, data.as_deref(), level)
      .iter()
      .map(|c| {
        let ground = if c.ok() {
          json!(round3(c.ground))
        } else {
          Value::Null
        };
        json!({"name": c.name, "position": v3(c.position), "ground": ground})
      })
      .collect();
    let mut r = json!({"replaces": level, "continues": checks});
    let own = self.own_level_bounds();
    if own.valid() {
      r["bounds"] = min_max(own);
    }
    for l in self.document_levels().iter() {
      if l.reference && l.name == level {
        let b = self.document_level_bounds(l.group);
        if b.valid() {
          r["original_bounds"] = min_max(b);
        }
      }
    }
    Ok(r)
  }

  pub(super) fn tool_save_project(&mut self, a: &Value) -> R {
    // a name: its folder in <library>/projects; a path: that file; else where it was saved
    let name = str_arg(a, "name");
    if !name.is_empty() {
      self
        .save_project_as(name)
        .map_err(|e| format!("cannot save: {e}"))?;
    } else {
      let mut path = match str_arg(a, "path") {
        "" => self.project_path.clone(),
        p => p.to_string(),
      };
      if path.is_empty() {
        return Err("name: the project's name (saved in <library>/projects/<name>/)".into());
      }
      if Path::new(&path).extension().is_none_or(|e| e != "glb") {
        path += ".glb";
      }
      self
        .save_project_files(&path)
        .map_err(|e| format!("cannot save: {e}"))?;
    }
    Ok(json!({"path": self.project_path}))
  }

  pub(super) fn tool_set_story(&mut self, a: &Value) -> R {
    let data = self
      .document_game_data()
      .filter(|d| d.has_story())
      .ok_or("no story view for this document (a new level has none)")?;
    let mut v = self.story.clone();
    // which level: the one opened (the default), or a level added, at its own point of the story
    let mut level = str_arg(a, "level").to_string();
    if level == self.scene.settings.level {
      level.clear();
    }
    if !level.is_empty() && !self.level_story_steps.contains_key(&level) {
      return Err("level: the level opened or a level added (get_status: document.levels)".into());
    }
    let set_step = |v: &mut crate::scene::story::StoryView, s: i32| {
      if level.is_empty() {
        v.step = s;
      } else {
        v.level_steps.insert(level.clone(), s);
      }
    };
    if let Some(e) = a.get("enabled").and_then(Value::as_bool) {
      v.enabled = e;
    }
    if let Some(s) = a.get("step").and_then(Value::as_i64) {
      set_step(&mut v, s as i32);
    }
    if let Some(s) = a.get("state").and_then(Value::as_i64) {
      let states = self.story_states(&level);
      if s < 1 || s as usize > states.len() {
        return Err(format!("state: 1 to {} for this level", states.len()));
      }
      set_step(&mut v, states[s as usize - 1]);
      v.enabled = true;
    }
    if let Some(node) = a.get("after_node").and_then(Value::as_str) {
      let found = (0..data.step_count().max(0) as usize)
        .rev()
        .find(|&i| data.node(i).name == node)
        .ok_or_else(|| format!("no task node '{node}'"))?;
      set_step(&mut v, found as i32 + 1);
      v.enabled = true;
    }
    self.story_level = level;
    self.set_story_view(v);
    Ok(self.remote_status()["story"].clone())
  }
}

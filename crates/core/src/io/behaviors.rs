//! The behaviors of the game's objects the editor sets, kept in a node's extras "og_behavior":
//! when a door opens, when a board vent throws Jak up, when an elevator leaves, whether an object
//! is there at all. The export writes them as generated GOAL code (<level>-obs.gc): a type of the
//! level deriving the object's own type, whose methods or states hold the choice, since OpenGOAL's
//! level builder cannot write the game's scripts (pair res lumps).
//!
//!   og_behavior.open      a door (com-airlock and its subtypes): "approach" (the default),
//!                         "locked", "always", "when" (once `when` holds); "distance": meters
//!                         Jak opens it from (0: the game's); "loads": a level (the rooms of a
//!                         house, a level of their own) loaded and shown when Jak comes to the
//!                         door, kept with the levels wanted already: the door opens once it is
//!   og_behavior.active    a bouncer (board vents, jump pads, awnings): when it throws Jak up
//!   og_behavior.leave     an elevator: when it leaves once ridden
//!   og_behavior.present   any object: whether it is there, checked when the level loads
//!
//! A condition is {"when": "always" | "never" | "task" | "pressed" | "gone", "arg": ...}: always,
//! never, once the task node `arg` of the story is closed, once the actor named `arg` was
//! activated (a floor switch pressed: its subtask-complete status), once it is gone (a crate
//! broken, a pickup taken: its dead status).

use std::fmt::Write as _;

use serde_json::{Value, json};

use super::game_data::GameData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BehaviorKind {
  Other,
  Door,
  Bouncer,
  Elevator,
}

pub fn behavior_kind(data: Option<&GameData>, etype: &str) -> BehaviorKind {
  let Some(data) = data else {
    return BehaviorKind::Other;
  };
  if data.derives_from(etype, "com-airlock") {
    BehaviorKind::Door
  } else if data.derives_from(etype, "bouncer") {
    BehaviorKind::Bouncer
  } else if data.derives_from(etype, "elevator") {
    BehaviorKind::Elevator
  } else {
    BehaviorKind::Other
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConditionKind {
  #[default]
  Always,
  Never,
  Task,
  Pressed,
  Gone,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Condition {
  pub kind: ConditionKind,
  pub arg: String,
}

// the names a script holds between quotes: task nodes and actor names
fn clean_name(s: &str) -> String {
  s.chars()
    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    .collect()
}

// a level's name as a symbol of the game: lower case letters, digits and '-' ("" for none)
fn level_symbol(s: &str) -> String {
  let out: String = s
    .chars()
    .map(|c| c.to_ascii_lowercase())
    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
    .collect();
  if out.starts_with(|c: char| c.is_ascii_alphabetic()) {
    out
  } else {
    String::new()
  }
}

// what the game's own doors test before opening (airlock.gc): no cutscene, Jak not grabbed
const K_DOOR_FREE: &str = "(not (or (scene-player?) (focus-test? *target* grabbed)))";

/// "always", "never", "task", "pressed", "gone"
pub fn condition_name(k: ConditionKind) -> &'static str {
  match k {
    ConditionKind::Always => "always",
    ConditionKind::Never => "never",
    ConditionKind::Task => "task",
    ConditionKind::Pressed => "pressed",
    ConditionKind::Gone => "gone",
  }
}

/// The condition `j` holds (`fallback` when it holds none: the C++ default is Always).
pub fn condition_of(j: &Value, fallback: Condition) -> Condition {
  if !j.is_object() {
    return fallback;
  }
  // a value of another kind (a project edited by hand, a client's mistake) reads as none
  let text = |key: &str| {
    j.get(key)
      .and_then(Value::as_str)
      .unwrap_or_default()
      .to_string()
  };
  let when = text("when");
  let kind = [
    ConditionKind::Always,
    ConditionKind::Never,
    ConditionKind::Task,
    ConditionKind::Pressed,
    ConditionKind::Gone,
  ]
  .into_iter()
  .find(|&k| when == condition_name(k))
  .unwrap_or(fallback.kind);
  let c = Condition {
    kind,
    arg: text("arg"),
  };
  // a condition on nothing named: as the fallback
  if matches!(
    c.kind,
    ConditionKind::Task | ConditionKind::Pressed | ConditionKind::Gone
  ) && clean_name(&c.arg).is_empty()
  {
    return fallback;
  }
  c
}

pub fn condition_json(c: &Condition) -> Value {
  let mut j = json!({ "when": condition_name(c.kind) });
  if !c.arg.is_empty() {
    j["arg"] = c.arg.clone().into();
  }
  j
}

/// The condition as a script of the game: "#t", "(task-closed? \"x\")"...
pub fn condition_script(c: &Condition) -> String {
  let a = clean_name(&c.arg);
  match c.kind {
    ConditionKind::Always => "#t".into(),
    ConditionKind::Never => "#f".into(),
    ConditionKind::Task => format!("(task-closed? \"{a}\")"),
    ConditionKind::Pressed => format!("(entity-status? \"{a}\" subtask-complete)"),
    ConditionKind::Gone => format!("(entity-status? \"{a}\" dead)"),
  }
}

/// The condition tested in GOAL: #t, #f, or the game's script evaluator on its script (a list:
/// script-eval takes nothing else).
pub fn condition_test(c: &Condition) -> String {
  match c.kind {
    ConditionKind::Always => "#t".into(),
    ConditionKind::Never => "#f".into(),
    _ => format!("(script-eval '{})", condition_script(c)),
  }
}

/// A name for the generated type of an actor: "<level>-<actor name>", lower case letters, digits
/// and '-' only.
pub fn behavior_type_name(level: &str, actor: &str) -> String {
  let mut out = String::new();
  for c in format!("{level}-{actor}").chars() {
    let l = c.to_ascii_lowercase();
    if l.is_ascii_alphanumeric() {
      out.push(l);
    } else if !out.is_empty() && !out.ends_with('-') {
      out.push('-');
    }
  }
  while out.ends_with('-') {
    out.pop();
  }
  out
}

/// The GOAL code of an actor of type `etype` with these behaviors, as a type `ty` deriving it;
/// "" when they need no code (the object as the game has it). `comment` heads the code.
pub fn behavior_code(
  data: Option<&GameData>,
  etype: &str,
  b: &Value,
  ty: &str,
  comment: &str,
) -> String {
  if !b.is_object() {
    return String::new();
  }
  let kind = behavior_kind(data, etype);
  let field = |key: &str| b.get(key).unwrap_or(&Value::Null);
  let mut body = String::new();

  // there or not, when the level loads
  let present = condition_of(field("present"), Condition::default());
  if present.kind != ConditionKind::Always && present.kind != ConditionKind::Never {
    let _ = write!(
      body,
      "(defmethod init-from-entity! ((this {ty}) (arg0 entity-actor))\n  (if {}\n      (call-parent-method this arg0)\n      (deactivate this)\n      )\n  (none)\n  )\n\n",
      condition_test(&present)
    );
  }

  if kind == BehaviorKind::Door {
    let open = field("open").as_str().unwrap_or("approach");
    let when = condition_of(field("when"), Condition::default());
    let distance = field("distance").as_f64().unwrap_or(0.0);
    let locked = open == "locked" || (open == "when" && when.kind == ConditionKind::Never);
    let loads = level_symbol(field("loads").as_str().unwrap_or_default());
    // without its on-notice script (the builder cannot write it), a door of the game never opens:
    // every door that opens says that the levels behind it are there, or loads the one behind it
    // (the rooms of a house, a level of their own) as the game's own doors do (the gun course's in
    // the port: want-load when Jak comes, opened once it is loaded, shown on entering)
    if !locked && !loads.is_empty() {
      let _ = write!(
        body,
        "(defmethod destination-loaded? ((this {ty}) (arg0 symbol))
  ;; Jak at the door (asked without a status): {loads} wanted with the levels wanted already, and shown
  (when (not arg0)
    (let ((have #f))
      (dotimes (i 10)
        (if (= (-> *load-state* want i name) '{loads})
            (set! have #t)
            )
        )
      (when (not have)
        (let ((names (new 'static 'boxed-array :type symbol :length 10 :allocated-length 10))
              (n 0)
              )
          (dotimes (i 10)
            (set! (-> names i) #f)
            )
          (dotimes (i 10)
            (when (-> *load-state* want i name)
              (set! (-> names n) (-> *load-state* want i name))
              (+! n 1)
              )
            )
          (when (< n 10)
            (set! (-> names n) '{loads})
            (want-levels *load-state* (-> names data))
            )
          )
        )
      )
    (dotimes (i 10)
      (if (and (= (-> *load-state* want i name) '{loads}) (not (-> *load-state* want i display?)))
          (want-display-level *load-state* '{loads} 'display)
          )
      )
    )
  (call-parent-method this arg0)
  )

"
      );
    } else if !locked {
      let _ = write!(
        body,
        "(defmethod destination-loaded? ((this {ty}) (arg0 symbol))\n  #t\n  )\n\n"
      );
    }
    if !locked {
      let test = if open == "always" {
        "(not #f)".to_string()
      } else if open == "when" && when.kind != ConditionKind::Always {
        format!("(and {K_DOOR_FREE} {})", condition_script(&when))
      } else {
        String::new()
      };
      let d = if open == "always" { 1000.0 } else { distance };
      if !test.is_empty() || d > 0.0 || !loads.is_empty() {
        let _ = write!(
          body,
          "(defmethod init-airlock! ((this {ty}))\n  (call-parent-method this)\n"
        );
        if !test.is_empty() {
          let _ = writeln!(body, "  (set! (-> this open-test) '{test})");
        }
        // the levels behind it, as its on-notice script would say: it opens once they are loaded
        if !loads.is_empty() {
          let _ = writeln!(body, "  (set! (-> this level-name) '(quote ({loads})))");
        }
        if d > 0.0 {
          let active = if open == "always" { d } else { d + 35.0 };
          let _ = write!(
            body,
            "  (set! (-> this open-distance 0) (meters {d:.2}))\n  (set! (-> this open-distance 1) (meters {d:.2}))\n  (set! (-> this active-distance 0) (meters {active:.2}))\n  (set! (-> this active-distance 1) (meters {active:.2}))\n"
          );
        }
        body.push_str("  this\n  )\n\n");
      }
    }
  }

  if kind == BehaviorKind::Bouncer {
    let active = condition_of(field("active"), Condition::default());
    if active.kind != ConditionKind::Always {
      let _ = write!(
        body,
        "(defstate idle ({ty})
  :virtual #t
  :event (behavior ((proc process) (argc int) (message symbol) (block event-message-block))
    (when {}
      (call-parent-state-handler event :type (function process int symbol event-message-block object)
                                 proc argc message block)
      )
    )
  )

",
        condition_test(&active)
      );
    }
  }

  if kind == BehaviorKind::Elevator {
    let leave = condition_of(field("leave"), Condition::default());
    if leave.kind != ConditionKind::Always {
      // its activate-test: a script (a list), "#f" written as one
      let test = if leave.kind == ConditionKind::Never {
        "(not #t)".to_string()
      } else {
        condition_script(&leave)
      };
      let _ = write!(
        body,
        "(defstate waiting ({ty})
  :virtual #t
  :enter (behavior ()
    (set! (-> self activate-test) '{test})
    (call-parent-state-handler enter)
    )
  )

"
      );
    }
  }

  if body.is_empty() {
    return String::new();
  }
  let mut out = String::new();
  if !comment.is_empty() {
    let _ = writeln!(out, ";; {comment}");
  }
  let _ = write!(out, "(deftype {ty} ({etype}) ())\n\n{body}");
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::io::game_data::tests::real_data;

  #[test]
  fn behaviors_as_the_games_code() {
    let data = real_data();
    let d = Some(&data);
    let fallback = Condition::default;
    let task = condition_of(
      &json!({"when": "task", "arg": "arena-fight-1-resolution"}),
      fallback(),
    );
    let none = condition_of(&json!({"when": "pressed", "arg": "\"; (evil)"}), fallback());
    let has = |code: &str, parts: &[&str]| parts.iter().all(|p| code.contains(p));
    let code = |etype: &str, b: Value, ty: &str| behavior_code(d, etype, &b, ty, "");
    let door = code("cty-door", json!({"open": "approach"}), "lv-door");
    let locked = code("cty-door", json!({"open": "locked"}), "lv-door");
    let always = code("cty-door", json!({"open": "always"}), "lv-door");
    let when = behavior_code(
      d,
      "cty-door",
      &json!({"open": "when", "when": condition_json(&task), "distance": 8}),
      "lv-door",
      "the door",
    );
    let vent = code(
      "industrial-board-vent",
      json!({"active": {"when": "never"}}),
      "lv-vent",
    );
    let elev = code(
      "elevator",
      json!({"leave": condition_json(&task)}),
      "lv-elev",
    );
    let there = code(
      "crate",
      json!({"present": {"when": "gone", "arg": "crate-9"}}),
      "lv-crate",
    );
    let plain = code("crate", json!({}), "lv-crate");
    // a door loading the rooms behind it (a level of their own): wanted with the others, shown,
    // the door waiting for it (no "#t")
    let rooms = code(
      "hip-door-a",
      json!({"open": "approach", "loads": "My Rooms!"}),
      "lv-home",
    );
    let loads = has(
      &rooms,
      &[
        "(set! (-> this level-name) '(quote (myrooms)))",
        "(want-levels *load-state* (-> names data))",
        "(want-display-level *load-state* 'myrooms 'display)",
        "(call-parent-method this arg0)",
      ],
    ) && !rooms.contains("  #t\n");
    assert_eq!(task.kind, ConditionKind::Task);
    assert_eq!(
      condition_script(&task),
      "(task-closed? \"arena-fight-1-resolution\")"
    );
    assert_eq!(
      condition_script(&none),
      "(entity-status? \"evil\" subtask-complete)"
    );
    assert_eq!(
      condition_name(condition_of(&condition_json(&task), fallback()).kind),
      "task"
    );
    assert!(has(
      &door,
      &[
        "(deftype lv-door (cty-door) ())",
        "(defmethod destination-loaded? ((this lv-door) (arg0 symbol))"
      ]
    ));
    assert!(locked.is_empty());
    assert!(has(
      &always,
      &["(set! (-> this open-test) '(not #f))", "(meters 1000.00)"]
    ));
    assert!(has(
      &when,
      &[
        ";; the door",
        "(task-closed? \"arena-fight-1-resolution\")",
        "(meters 8.00)",
        "(meters 43.00)",
        "(call-parent-method this)",
      ]
    ));
    assert!(has(
      &vent,
      &[
        "(defstate idle (lv-vent)",
        "(when #f",
        "call-parent-state-handler event"
      ]
    ));
    assert!(has(
      &elev,
      &[
        "(defstate waiting (lv-elev)",
        "(set! (-> self activate-test) '(task-closed?",
        "call-parent-state-handler enter"
      ]
    ));
    assert!(has(
      &there,
      &[
        "(defmethod init-from-entity! ((this lv-crate) (arg0 entity-actor))",
        "(entity-status? \"crate-9\" dead)",
        "(deactivate this)",
      ]
    ));
    assert!(plain.is_empty());
    assert_eq!(
      behavior_type_name("my-level", "Cty Door_1"),
      "my-level-cty-door-1"
    );
    assert!(loads, "behaviors: a door loading the rooms behind it");
    assert_eq!(behavior_kind(d, "wascity-awning-a"), BehaviorKind::Bouncer);
    assert_eq!(behavior_kind(None, "cty-door"), BehaviorKind::Other);
  }
}

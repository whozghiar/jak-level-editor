//! Remote control of the editor (src/app/remote.h, remote.cpp), used by its MCP server
//! (crates/mcp): the editor listens on a local port (127.0.0.1 only) for requests, one JSON object
//! per line,
//!
//!   {"id": 1, "tool": "open_level", "args": {"name": "hiphog"}}
//!
//! and answers each on its connection, one line too:
//!
//!   {"id": 1, "ok": true, "result": {...}}
//!   {"id": 1, "ok": true, "result": {...}, "image": "<PNG in base64>"}   (a capture)
//!   {"id": 1, "ok": false, "error": "..."}
//!
//! The editor carries the requests out under its lock, like the user's actions: they are shown as
//! they happen and can be undone. A tool that waits for something (a level loading, a job, a
//! picture of the view) is asked again every few milliseconds, the lock let go in between.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Editor, SharedEditor, lock};

pub const K_REMOTE_PORT: u16 = 47821;

/// how long a tool may wait (the MCP server gives the slow ones 300 s)
const K_REMOTE_WAIT: Duration = Duration::from_secs(300);
const K_REMOTE_POLL: Duration = Duration::from_millis(5);

/// What a tool answers: now (the answer line without its id), or later: the closure is called
/// again under the lock every few milliseconds until it answers (another wait, or the answer).
pub enum Reply {
  Now(Value),
  Wait(Waiter),
}

pub type Waiter = Box<dyn FnMut(&mut Editor) -> Option<Reply> + Send>;

impl From<Result<Value, String>> for Reply {
  fn from(r: Result<Value, String>) -> Self {
    Reply::Now(match r {
      Ok(result) => json!({"ok": true, "result": result}),
      Err(error) => json!({"ok": false, "error": error}),
    })
  }
}

impl Reply {
  pub fn wait(f: impl FnMut(&mut Editor) -> Option<Reply> + Send + 'static) -> Reply {
    Reply::Wait(Box::new(f))
  }
}

impl Editor {
  /// One step of a request under the lock (a tool, or its wait asked again): what it changed
  /// told to the interface, a panic answered as an error.
  fn remote_step(
    &mut self,
    tool: &str,
    step: impl FnOnce(&mut Editor) -> Option<Reply>,
  ) -> Option<Reply> {
    let before = self.watch();
    self.scene.begin_frame();
    let r = catch_unwind(AssertUnwindSafe(|| step(self))).unwrap_or_else(|e| {
      let what = e
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| e.downcast_ref::<&str>().copied())
        .unwrap_or("panic");
      Some(Err(format!("error in {tool}: {what}")).into())
    });
    self.after_change(&before);
    r
  }
}

/// A tool run to its answer: under the lock, then its waits asked again without holding it in
/// between. The answer line without its id.
pub fn run_tool(editor: &Mutex<Editor>, tool: &str, args: &Value) -> Value {
  let mut reply = lock(editor)
    .remote_step(tool, |ed| Some(ed.remote_tool(tool, args)))
    .unwrap();
  let start = Instant::now();
  loop {
    match reply {
      Reply::Now(answer) => return answer,
      Reply::Wait(mut wait) => {
        if start.elapsed() > K_REMOTE_WAIT {
          return json!({"ok": false, "error": format!("{tool}: the editor did not finish in time")});
        }
        std::thread::sleep(K_REMOTE_POLL);
        reply = lock(editor)
          .remote_step(tool, |ed| wait(ed))
          .unwrap_or(Reply::Wait(wait));
      }
    }
  }
}

/// Listens on 127.0.0.1:`port` (this computer only): a thread accepts the connections, one thread
/// reads each. The error when the port is taken.
pub fn start_remote(editor: &SharedEditor, port: u16) -> Result<(), String> {
  // ponytail: std binds without SO_EXCLUSIVEADDRUSE (the C++ set it on Windows: no other program
  // can take the port while the editor has it); a socket of our own if that matters
  let listener = TcpListener::bind(("127.0.0.1", port))
    .map_err(|_| format!("port {port} already in use (another editor?)"))?;
  let editor = Arc::downgrade(editor);
  std::thread::spawn(move || {
    for stream in listener.incoming() {
      let (Ok(stream), Some(editor)) = (stream, editor.upgrade()) else {
        continue;
      };
      std::thread::spawn(move || serve_client(&editor, stream));
    }
  });
  Ok(())
}

/// the editor's count of assistants connected (the status bar's "Assistant connected")
fn count_client(editor: &Mutex<Editor>, connected: bool) {
  let mut ed = lock(editor);
  let before = ed.watch();
  if connected {
    ed.remote_clients += 1;
  } else {
    ed.remote_clients = ed.remote_clients.saturating_sub(1);
  }
  ed.after_change(&before);
}

fn serve_client(editor: &Mutex<Editor>, stream: TcpStream) {
  let _ = stream.set_nodelay(true);
  let Ok(mut out) = stream.try_clone() else {
    return;
  };
  count_client(editor, true);
  for line in BufReader::new(stream).split(b'\n') {
    let Ok(line) = line else { break };
    let answer = answer_line(editor, &String::from_utf8_lossy(&line));
    if let Some(a) = answer
      && writeln!(out, "{a}").is_err()
    {
      break;
    }
  }
  count_client(editor, false);
}

/// The answer to one line of a client (None for an empty line)
pub fn answer_line(editor: &Mutex<Editor>, line: &str) -> Option<Value> {
  if line.trim().is_empty() {
    return None;
  }
  let j: Value = serde_json::from_str(line).unwrap_or_default();
  let id = j.get("id").cloned().unwrap_or(Value::Null);
  let Some(tool) = j.get("tool").and_then(Value::as_str) else {
    return Some(json!({
      "id": id,
      "ok": false,
      "error": "not a request: {\"id\": ..., \"tool\": \"...\", \"args\": {...}}",
    }));
  };
  let args = j
    .get("args")
    .filter(|a| a.is_object())
    .cloned()
    .unwrap_or_else(|| json!({}));
  let mut answer = run_tool(editor, tool, &args);
  answer["id"] = id;
  Some(answer)
}

#[cfg(test)]
mod tests {
  use super::*;

  // the remote control of the MCP server: a request and its answer, one line each (selftest.cpp)
  #[test]
  fn a_request_and_its_answer_over_the_local_connection() {
    let editor = Editor::new().shared(None);
    // a port free now (47899 in the C++ selftest: an editor of the tests may hold it)
    let port = TcpListener::bind(("127.0.0.1", 0))
      .unwrap()
      .local_addr()
      .unwrap()
      .port();
    start_remote(&editor, port).unwrap();
    assert!(
      start_remote(&editor, port)
        .unwrap_err()
        .contains("already in use")
    );
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut ask = |line: &str| -> Value {
      writeln!(s, "{line}").unwrap();
      let mut a = String::new();
      r.read_line(&mut a).unwrap();
      serde_json::from_str(&a).unwrap()
    };
    let a = ask(r#"{"id": 7, "tool": "get_status", "args": {}}"#);
    assert_eq!(a["id"], 7);
    assert_eq!(a["ok"], true);
    assert_eq!(a["result"]["document"]["open"], false);
    let a = ask(r#"{"id": 8, "tool": "no_such_tool"}"#);
    assert_eq!(a["id"], 8);
    assert_eq!(a["ok"], false);
    assert_eq!(a["error"], "unknown tool 'no_such_tool'");
    let a = ask("not json");
    assert_eq!(a["ok"], false);
    assert!(a["error"].as_str().unwrap().starts_with("not a request"));
    // the status bar knows an assistant is there
    assert_eq!(lock(&editor).remote_clients, 1);
    assert_eq!(lock(&editor).get_state()["remote_clients"], 1);
  }
}

//! Remote control of the editor (src/app/remote.h, remote.cpp), used by its MCP server
//! (crates/mcp): the editor listens on a local port (127.0.0.1 only) for requests, one JSON object
//! per line, each carrying the token the editor wrote to `remote.json` in its preferences' folder
//! (`{"port": 47821, "token": "..."}`) when it started listening,
//!
//!   {"id": 1, "tool": "open_level", "args": {"name": "hiphog"}, "token": "..."}
//!
//! and answers each on its connection, one line too:
//!
//!   {"id": 1, "ok": true, "result": {...}}
//!   {"id": 1, "ok": true, "result": {...}, "image": "<PNG in base64>"}   (a capture)
//!   {"id": 1, "ok": false, "error": "..."}
//!
//! It listens only while the user lets an assistant control the editor (Prefs::remote_control,
//! File > Settings), or for the session the MCP server started the editor for (--remote-port). A
//! request without the token is refused (a web page sending to the port cannot read the user's
//! files), and a line that is not a JSON object (an HTTP header) closes the connection.
//!
//! The editor carries the requests out under its lock, like the user's actions: they are shown as
//! they happen and can be undone. A tool that waits for something (a level loading, a job, a
//! picture of the view) is asked again every few milliseconds, the lock let go in between.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Editor, SharedEditor, lock};
use crate::i18n::tr;
use crate::prefs::Prefs;
use crate::{log_info, log_warn};

pub const K_REMOTE_PORT: u16 = 47821;

/// the file telling the MCP server where the editor listens and the token of its requests
pub const K_REMOTE_FILE: &str = "remote.json";

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

/// The remote control listening (Editor::remote): dropped, it stops listening (the port closed),
/// its connections end and its remote.json goes.
pub struct Remote {
  pub port: u16,
  token: String,
  file: PathBuf,
  stop: Arc<AtomicBool>,
}

impl Drop for Remote {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    // another editor may have written its own since
    if std::fs::read_to_string(&self.file).is_ok_and(|t| t.contains(&self.token)) {
      let _ = std::fs::remove_file(&self.file);
    }
  }
}

/// `<preferences' folder>/remote.json`
pub fn remote_file() -> PathBuf {
  Path::new(&Prefs::dir()).join(K_REMOTE_FILE)
}

fn new_token() -> Result<String, String> {
  let mut bytes = [0u8; 16];
  getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
  Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Listens on 127.0.0.1:`port` (this computer only), its port and a new token written to `file`
/// (remote_file): a thread accepts the connections, one thread reads each. The error when the port
/// is taken.
pub fn start_remote(editor: &SharedEditor, port: u16, file: &Path) -> Result<Remote, String> {
  // ponytail: std binds without SO_EXCLUSIVEADDRUSE (the C++ set it on Windows: no other program
  // can take the port while the editor has it); a socket of our own if that matters
  let listener = TcpListener::bind(("127.0.0.1", port))
    .map_err(|_| format!("port {port} already in use (another editor?)"))?;
  // polled, so that the thread sees `stop` and the port closes with it
  listener.set_nonblocking(true).map_err(|e| e.to_string())?;
  let token = new_token()?;
  if let Some(dir) = file.parent() {
    let _ = std::fs::create_dir_all(dir);
  }
  std::fs::write(file, json!({"port": port, "token": token}).to_string())
    .map_err(|e| format!("{}: {e}", file.display()))?;
  let remote = Remote {
    port,
    token: token.clone(),
    file: file.to_path_buf(),
    stop: Arc::default(),
  };
  let (editor, stop) = (Arc::downgrade(editor), remote.stop.clone());
  std::thread::spawn(move || {
    while !stop.load(Ordering::Relaxed) {
      let Ok((stream, _)) = listener.accept() else {
        std::thread::sleep(Duration::from_millis(100));
        continue;
      };
      let Some(editor) = editor.upgrade() else {
        break;
      };
      let _ = stream.set_nonblocking(false);
      let (token, stop) = (token.clone(), stop.clone());
      std::thread::spawn(move || serve_client(&editor, stream, &token, &stop));
    }
  });
  Ok(remote)
}

impl Editor {
  /// The remote control started on `port` (its failure logged: the port taken by another editor)
  pub fn start_remote_control(&mut self, port: u16) {
    let Some(me) = self.me.upgrade() else {
      return;
    };
    match start_remote(&me, port, &remote_file()) {
      Ok(r) => {
        self.remote = Some(r);
        log_info!(tr("log.remote_ready"), port);
      }
      Err(e) => log_warn!(tr("log.remote_failed"), e),
    }
  }

  /// remote_state: `on` (the preference), `port` (where it listens, null when it does not),
  /// `session` (the MCP server started the editor with --remote-port), `file` (remote.json)
  pub fn remote_state(&self) -> Value {
    json!({
      "on": Prefs::get().remote_control,
      "port": self.remote.as_ref().map(|r| r.port),
      "session": self.remote_session.is_some(),
      "file": crate::io::asset_library::from_path(&remote_file()),
    })
  }

  /// set_remote_control {on}: the preference kept, the remote control started or stopped now
  pub fn set_remote_control(&mut self, on: bool) -> Value {
    self.save_prefs(|p| p.remote_control = on);
    if !on {
      self.remote = None;
    } else if self.remote.is_none() {
      self.start_remote_control(self.remote_session.unwrap_or(K_REMOTE_PORT));
    }
    self.remote_state()
  }
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

fn serve_client(editor: &Mutex<Editor>, stream: TcpStream, token: &str, stop: &AtomicBool) {
  let _ = stream.set_nodelay(true);
  // a moment at most between two looks at `stop`
  let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
  let Ok(mut out) = stream.try_clone() else {
    return;
  };
  let mut reader = BufReader::new(stream);
  let (mut line, mut counted) = (Vec::new(), false);
  while !stop.load(Ordering::Relaxed) {
    match reader.read_until(b'\n', &mut line) {
      Ok(0) => break,
      Ok(_) => {}
      // the bytes of a line read so far stay in `line`
      Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
      Err(_) => break,
    }
    let text = String::from_utf8_lossy(&line).into_owned();
    line.clear();
    if text.trim().is_empty() {
      continue;
    }
    // not a request (a web page's HTTP header): the connection closed
    let Ok(request @ Value::Object(_)) = serde_json::from_str::<Value>(&text) else {
      break;
    };
    if request.get("token").and_then(Value::as_str) != Some(token) {
      let id = request.get("id").cloned().unwrap_or(Value::Null);
      let refused = json!({"id": id, "ok": false,
        "error": "refused: the request has no token, or not the one of the editor's remote.json"});
      let _ = writeln!(out, "{refused}");
      break;
    }
    if !counted {
      counted = true;
      count_client(editor, true);
    }
    if writeln!(out, "{}", answer(editor, &request)).is_err() {
      break;
    }
  }
  if counted {
    count_client(editor, false);
  }
}

/// The answer to a request, its id kept
pub fn answer(editor: &Mutex<Editor>, j: &Value) -> Value {
  let id = j.get("id").cloned().unwrap_or(Value::Null);
  let Some(tool) = j.get("tool").and_then(Value::as_str) else {
    return json!({
      "id": id,
      "ok": false,
      "error": "not a request: {\"id\": ..., \"tool\": \"...\", \"args\": {...}, \"token\": \"...\"}",
    });
  };
  let args = j
    .get("args")
    .filter(|a| a.is_object())
    .cloned()
    .unwrap_or_else(|| json!({}));
  let mut answer = run_tool(editor, tool, &args);
  answer["id"] = id;
  answer
}

#[cfg(test)]
mod tests {
  use super::*;

  // the remote control of the MCP server: a request and its answer, one line each (selftest.cpp)
  #[test]
  fn a_request_and_its_answer_over_the_local_connection() {
    let editor = Editor::new().shared(None);
    let file = crate::io::test_library::temp_dir("remote").join(K_REMOTE_FILE);
    // a port free now (47899 in the C++ selftest: an editor of the tests may hold it)
    let port = TcpListener::bind(("127.0.0.1", 0))
      .unwrap()
      .local_addr()
      .unwrap()
      .port();
    let remote = start_remote(&editor, port, &file).unwrap();
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert!(written["port"] == port && written["token"] == remote.token.as_str());
    assert!(
      start_remote(&editor, port, &file)
        .err()
        .is_some_and(|e| e.contains("already in use"))
    );
    let connect = || {
      let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
      s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
      let r = BufReader::new(s.try_clone().unwrap());
      (s, r)
    };
    let (mut s, mut r) = connect();
    let token = remote.token.clone();
    let mut ask = |line: &str| -> Value {
      writeln!(s, "{line}").unwrap();
      let mut a = String::new();
      r.read_line(&mut a).unwrap();
      serde_json::from_str(&a).unwrap_or(Value::Null)
    };
    let a = ask(&format!(
      r#"{{"id": 7, "tool": "get_status", "args": {{}}, "token": "{token}"}}"#
    ));
    assert_eq!(a["id"], 7);
    assert_eq!(a["ok"], true);
    assert_eq!(a["result"]["document"]["open"], false);
    let a = ask(&format!(
      r#"{{"id": 8, "tool": "no_such_tool", "token": "{token}"}}"#
    ));
    assert_eq!(a["id"], 8);
    assert_eq!(a["ok"], false);
    assert_eq!(a["error"], "unknown tool 'no_such_tool'");
    // the status bar knows an assistant is there
    assert_eq!(lock(&editor).remote_clients, 1);
    assert_eq!(lock(&editor).get_state()["remote_clients"], 1);
    // not a JSON object (a web page's HTTP request): the connection closed, nothing answered
    assert!(ask("POST / HTTP/1.1").is_null());

    // without the token, or another: refused, the connection closed
    let (mut s, mut r) = connect();
    writeln!(s, r#"{{"id": 9, "tool": "get_status", "token": "nope"}}"#).unwrap();
    let mut a = String::new();
    r.read_line(&mut a).unwrap();
    assert!(a.contains("refused"), "{a}");
    a.clear();
    assert_eq!(r.read_line(&mut a).unwrap_or(0), 0, "closed");

    // stopped: the port closed, remote.json gone
    drop(remote);
    std::thread::sleep(Duration::from_millis(300));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err() && !file.exists());
  }
}

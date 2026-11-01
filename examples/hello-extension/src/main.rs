//! A small extension for Hesewake Notch (see docs/extensions.md). It shows a list with filter tabs, opens a detail page
//! for a row, has a second page with a stepper, and sends a toast, a badge and a status when asked.
//!
//! It answers what the notch sends and never writes on its own schedule: a plain (synchronous) pipe handle cannot be
//! read and written from two threads at once, so this example stays simple. An extension that must send by itself,
//! for example after polling a server, should use overlapped I/O the way the notch does.

use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};

const MAX_FRAME: usize = 64 * 1024;

fn send(pipe: &mut File, v: Value) -> io::Result<()> {
    let body = v.to_string().into_bytes();
    pipe.write_all(&(body.len() as u32).to_le_bytes())?;
    pipe.write_all(&body)?;
    pipe.flush()
}

fn recv(pipe: &mut File) -> io::Result<Value> {
    let mut len = [0u8; 4];
    pipe.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad frame length"));
    }
    let mut body = vec![0u8; n];
    pipe.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The notch puts the pipe's name in the environment of the programs it starts. (An extension started by anything else
/// builds it from the user's SID: `\\.\pipe\hesewake-notch-ext-<SID>`.)
fn open_pipe() -> io::Result<File> {
    let name = std::env::var("HESEWAKE_NOTCH_PIPE").map_err(|_| io::Error::new(io::ErrorKind::NotFound, "HESEWAKE_NOTCH_PIPE is not set"))?;
    OpenOptions::new().read(true).write(true).open(name)
}

struct Hello {
    /// Grows with every `slide` and `detail`; the notch drops one that is not newer than the last.
    rev: u64,
    tab: String,
    /// Which day the second page shows (0 = today).
    day: i32,
    events: u32,
}

impl Hello {
    fn next_rev(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    fn status(&self) -> Value {
        json!({"t": "status", "text": format!("Hello, {} events", self.events)})
    }

    /// The home page: twelve items, filtered by the tab.
    fn home(&mut self) -> Value {
        let rows: Vec<Value> = (1..=12)
            .filter(|n| match self.tab.as_str() {
                "odd" => n % 2 == 1,
                "even" => n % 2 == 0,
                _ => true,
            })
            .map(|n| json!({"id": n.to_string(), "text": format!("Item {n}: something to look at"), "sub": if n % 2 == 1 { "odd" } else { "even" }, "mark": n == 1}))
            .collect();
        json!({
            "t": "slide", "rev": self.next_rev(), "title": "Hel|lo",
            "tabs": [{"id": "all", "label": "All", "count": 12}, {"id": "odd", "label": "Odd", "count": 6}, {"id": "even", "label": "Even", "count": 6}],
            "tab": self.tab,
            "actions": [{"id": "notify", "label": "Notify"}, {"id": "report", "label": "Report"}],
            "rows": rows,
            "foot": "Click a row for its page"
        })
    }

    fn detail(&mut self, id: &str) -> Value {
        json!({
            "t": "detail", "rev": self.next_rev(), "for": id, "title": format!("Item {id}"),
            "meta": [{"k": "Number", "v": id}, {"k": "Kind", "v": if id.parse::<u32>().unwrap_or(0) % 2 == 1 { "odd" } else { "even" }}],
            "body": "This page was sent by the example extension. The notch draws it; the extension only chose the words.\nOpen goes to the repository, Copy puts the title on the clipboard, and More is passed back to the extension.",
            "notes": [{"who": "Example", "when": "today", "text": "Notes are plain text too."}],
            "buttons": [
                {"id": "open", "label": "Open", "link": "https://github.com/dirgaastya/hesewake-notch"},
                {"id": "copy", "label": "Copy", "copy": format!("Item {id}")},
                {"id": "more", "label": "More"}
            ],
            "foot": "Back returns to the list"
        })
    }

    /// The second page: two sections and a stepper over the days.
    fn report(&mut self) -> Value {
        let day = match self.day {
            0 => "today".to_string(),
            -1 => "yesterday".to_string(),
            n => format!("{} days ago", -n),
        };
        let rows = |n: i32, p: &str| (0..n).map(|i| json!({"id": format!("{p}{i}"), "text": format!("{p} #{i} on {day}"), "sub": "done"})).collect::<Vec<_>>();
        json!({
            "t": "slide", "rev": self.next_rev(), "title": "Re|port", "back": true,
            "stepper": {"label": day, "prev": self.day > -6, "next": self.day < 0},
            "actions": [{"id": "copy", "label": "Copy", "copy": format!("Report for {day}")}],
            "sections": [{"title": "Worked on", "rows": rows(3, "w")}, {"title": "Queue", "rows": rows(2, "q")}]
        })
    }
}

fn main() -> io::Result<()> {
    let mut pipe = open_pipe()?;
    send(&mut pipe, json!({"t": "hello", "proto": 1, "id": "hello-extension", "name": "Hello", "version": env!("CARGO_PKG_VERSION")}))?;
    let welcome = recv(&mut pipe)?;
    if welcome["t"] != "welcome" {
        // `reject` says why (`disabled`, `unknown`, `proto`, `busy`); nothing to do but leave
        return Ok(());
    }
    let mut me = Hello { rev: 0, tab: "all".into(), day: 0, events: 0 };
    let first = me.home();
    send(&mut pipe, first)?;
    send(&mut pipe, me.status())?;
    loop {
        let msg = match recv(&mut pipe) {
            Ok(m) => m,
            Err(_) => return Ok(()), // the notch closed the pipe
        };
        me.events += 1;
        let id = msg["id"].as_str().unwrap_or("").to_string();
        match msg["t"].as_str().unwrap_or("") {
            "quit" => {
                send(&mut pipe, json!({"t": "bye"}))?;
                return Ok(());
            }
            "visible" if msg["on"] == true => {
                // the slide was opened: whatever was new is seen
                send(&mut pipe, json!({"t": "badge", "on": false}))?;
            }
            "tab" => {
                me.tab = id;
                let page = me.home();
                send(&mut pipe, page)?;
            }
            "row" => {
                let page = me.detail(&id);
                send(&mut pipe, page)?;
            }
            "back" => {
                let page = me.home();
                send(&mut pipe, page)?;
            }
            "step" => {
                me.day = (me.day + msg["dir"].as_i64().unwrap_or(0) as i32).clamp(-6, 0);
                let page = me.report();
                send(&mut pipe, page)?;
            }
            "action" => match id.as_str() {
                "report" => {
                    me.day = 0;
                    let page = me.report();
                    send(&mut pipe, page)?;
                }
                "notify" => {
                    send(&mut pipe, json!({"t": "toast", "title": "Hello from the example", "sub": "A toast sent on request", "count": 1}))?;
                    send(&mut pipe, json!({"t": "badge", "on": true}))?;
                }
                _ => {}
            },
            "config" => {
                // a real extension opens its own settings window here
                send(&mut pipe, json!({"t": "toast", "title": "Hello has no settings", "sub": "Nothing to configure"}))?;
            }
            _ => {}
        }
        send(&mut pipe, me.status())?;
    }
}

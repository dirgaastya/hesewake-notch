//! Extension host (docs/extensions.md): the named pipe that extensions connect to, who may connect, and what comes in.
//! The pipe only lets the current user in. A connection must say hello, and its program must be the `exe` of an enabled
//! manifest; after that its messages are checked by `extproto` and queued for the UI thread (`take_events`). The UI
//! thread answers with `send`. Reading and writing each run on a thread of their own, so an extension that stops
//! reading or never answers can only stall its own connection.

#![allow(dead_code)] // wired up in the next steps of the extension host

use super::*;
use crate::extproto::{self as proto, FromExt, Parsed};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// Message to the window: something is waiting in `Host::take_events`.
pub(crate) const WM_EXT: u32 = WM_APP + 20;

/// How long a new connection has to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);
/// This many unreadable messages in a row close the connection.
const BAD_IN_A_ROW: u32 = 3;

/// Something that happened on a connection, for the UI thread.
#[derive(Debug, PartialEq)]
pub(crate) enum Evt {
    Connected { id: String },
    Msg { id: String, msg: FromExt },
    /// The connection ended; `why` is `closed`, `bye`, `bad data` or `stopped`.
    Gone { id: String, why: &'static str },
}

struct Conn {
    tx: Sender<Vec<u8>>,
    h: isize,
    generation: u64,
}

struct Allowed {
    exe: String,
    enabled: bool,
}

/// `C:/Tools/A.exe` and `c:\tools\a.exe` are the same program.
fn norm(p: &str) -> String {
    p.replace('/', "\\").to_ascii_lowercase()
}

pub(crate) struct Host {
    allowed: Mutex<HashMap<String, Allowed>>,
    conns: Mutex<HashMap<String, Conn>>,
    events: Mutex<Vec<Evt>>,
    /// What `welcome` tells: app version, language, accent.
    info: Mutex<(String, String, [u8; 3])>,
    next_generation: AtomicU64,
    hello_wait: Duration,
}

impl Host {
    pub(crate) fn new(app: &str) -> Arc<Host> {
        Host::with_hello_wait(app, HELLO_WAIT)
    }

    fn with_hello_wait(app: &str, hello_wait: Duration) -> Arc<Host> {
        Arc::new(Host {
            allowed: Mutex::new(HashMap::new()),
            conns: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            info: Mutex::new((app.to_string(), "en".to_string(), [90, 170, 255])),
            next_generation: AtomicU64::new(1),
            hello_wait,
        })
    }

    /// The manifests found: (id, exe, enabled). A connection from anything else is turned away. A connected extension
    /// that is no longer enabled (or whose exe changed) is disconnected.
    pub(crate) fn set_allowed(&self, list: Vec<(String, String, bool)>) {
        let map: HashMap<String, Allowed> = list.into_iter().map(|(id, exe, enabled)| (id, Allowed { exe: norm(&exe), enabled })).collect();
        let stale: Vec<String> = self
            .conns
            .lock()
            .map(|c| c.keys().filter(|id| !map.get(*id).is_some_and(|a| a.enabled)).cloned().collect())
            .unwrap_or_default();
        if let Ok(mut a) = self.allowed.lock() {
            *a = map;
        }
        for id in stale {
            self.disconnect(&id);
        }
    }

    pub(crate) fn set_info(&self, lang: &str, accent: [u8; 3]) {
        if let Ok(mut i) = self.info.lock() {
            i.1 = lang.to_string();
            i.2 = accent;
        }
    }

    pub(crate) fn take_events(&self) -> Vec<Evt> {
        self.events.lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default()
    }

    pub(crate) fn connected(&self, id: &str) -> bool {
        self.conns.lock().is_ok_and(|c| c.contains_key(id))
    }

    /// Queue a message (already a frame, see `extproto`) for an extension. False if it is not connected.
    pub(crate) fn send(&self, id: &str, frame: Vec<u8>) -> bool {
        self.conns.lock().ok().and_then(|c| c.get(id).map(|c| c.tx.send(frame).is_ok())).unwrap_or(false)
    }

    /// Tell it to quit and let go of the connection (the reading thread notices and cleans up).
    pub(crate) fn disconnect(&self, id: &str) {
        if let Ok(mut c) = self.conns.lock() {
            if let Some(conn) = c.remove(id) {
                // the writing thread sends the queue (quit last), then wakes the reader, which cleans up
                let _ = conn.tx.send(proto::quit());
            }
        }
    }

    fn push(&self, hwnd: isize, e: Evt) {
        if let Ok(mut q) = self.events.lock() {
            q.push(e);
        }
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_EXT, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Create the pipe and accept connections on a thread of its own. The error says why there is no pipe.
    pub(crate) fn serve(self: &Arc<Host>, pipe: &str, hwnd: isize) -> std::result::Result<(), String> {
        let sd = unsafe { user_only_descriptor() }.ok_or("could not build the pipe's access list")?;
        let first = unsafe { create_pipe(pipe, true, sd) }.ok_or("could not create the pipe (is another notch running?)")?;
        let (host, pipe, first) = (self.clone(), pipe.to_string(), first.0 as isize);
        std::thread::spawn(move || {
            let mut h = HANDLE(first as *mut c_void);
            loop {
                unsafe { accept(h) };
                // the next instance exists before this one is handled, so there is always a pipe to find
                let next = loop {
                    if let Some(n) = unsafe { create_pipe(&pipe, false, sd) } {
                        break n;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                };
                let (host, cur) = (host.clone(), h.0 as isize);
                std::thread::spawn(move || host.handle(HANDLE(cur as *mut c_void), hwnd));
                h = next;
            }
        });
        Ok(())
    }

    fn reject(&self, h: HANDLE, reason: &str) {
        write_all(h, &proto::reject(reason));
    }

    /// One connection, from hello to the end.
    fn handle(&self, h: HANDLE, hwnd: isize) {
        let mut fb = proto::FrameBuf::default();
        let mut buf = vec![0u8; 8192];
        let deadline = Instant::now() + self.hello_wait;
        // hello first, within the time limit (the wait polls, so a silent client cannot hold a thread for ever)
        let hello = loop {
            match fb.next() {
                Ok(Some(p)) => break p,
                Ok(None) => {}
                Err(_) => return close(h),
            }
            if !wait_for_data(h, deadline) {
                return close(h);
            }
            match read_some(h, &mut buf) {
                Some(n) => fb.push(&buf[..n]),
                None => return close(h),
            }
        };
        let FromExt::Hello { proto: ext_proto, id, .. } = (match proto::parse(&hello) {
            Parsed::Msg(m) => m,
            _ => return close(h),
        }) else {
            return close(h);
        };
        let exe = unsafe { client_pid(h) }.and_then(image_path).map(|p| norm(&p));
        let verdict = {
            let allowed = self.allowed.lock();
            match allowed.as_ref().ok().and_then(|a| a.get(&id)) {
                None => Err("unknown"),
                Some(a) if !a.enabled => Err("disabled"),
                Some(a) if exe.as_deref() != Some(a.exe.as_str()) => Err("unknown"),
                Some(_) => proto::negotiate(ext_proto),
            }
        };
        if let Err(reason) = verdict {
            self.reject(h, reason);
            return close(h);
        }
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel::<Vec<u8>>();
        {
            let Ok(mut conns) = self.conns.lock() else { return close(h) };
            if conns.contains_key(&id) {
                drop(conns);
                self.reject(h, "busy");
                return close(h);
            }
            let (app, lang, accent) = self.info.lock().map(|i| i.clone()).unwrap_or_default();
            let _ = tx.send(proto::welcome(&app, &lang, accent));
            conns.insert(id.clone(), Conn { tx, h: h.0 as isize, generation });
        }
        let hraw = h.0 as isize;
        let writer = std::thread::spawn(move || {
            let h = HANDLE(hraw as *mut c_void);
            while let Ok(frame) = rx.recv() {
                if !write_all(h, &frame) {
                    break;
                }
            }
            // nothing more to send: wake the reader if it still waits (a `disconnect` ends up here)
            unsafe {
                let _ = CancelIoEx(h, None);
            }
        });
        self.push(hwnd, Evt::Connected { id: id.clone() });

        let (mut limit, mut gate, mut rev, mut bad) = (proto::RateLimit::new(Instant::now()), proto::ToastGate::default(), None, 0u32);
        let why = 'conn: loop {
            loop {
                match fb.next() {
                    Err(_) => break 'conn "bad data",
                    Ok(None) => break,
                    Ok(Some(payload)) => {
                        if !limit.allow(Instant::now()) {
                            continue;
                        }
                        match proto::parse(&payload) {
                            Parsed::Bad => {
                                bad += 1;
                                if bad >= BAD_IN_A_ROW {
                                    break 'conn "bad data";
                                }
                            }
                            Parsed::Ignored => bad = 0,
                            Parsed::Msg(m) => {
                                bad = 0;
                                let keep = match &m {
                                    FromExt::Slide(s) => proto::newer(&mut rev, s.rev),
                                    FromExt::Detail(d) => proto::newer(&mut rev, d.rev),
                                    FromExt::Toast { .. } => gate.allow(Instant::now()),
                                    FromExt::Hello { .. } => false,
                                    _ => true,
                                };
                                if keep {
                                    let bye = m == FromExt::Bye;
                                    self.push(hwnd, Evt::Msg { id: id.clone(), msg: m });
                                    if bye {
                                        break 'conn "bye";
                                    }
                                }
                            }
                        }
                    }
                }
            }
            match read_some(h, &mut buf) {
                Some(n) => fb.push(&buf[..n]),
                None => break "closed",
            }
        };

        // Let go: out of the table first (a `disconnect` may have done it already), then wake and end the writer.
        let ours = match self.conns.lock() {
            Ok(mut c) if c.get(&id).is_some_and(|c| c.generation == generation) => c.remove(&id),
            _ => None,
        };
        let why = if ours.is_none() && why == "closed" { "stopped" } else { why };
        drop(ours);
        unsafe {
            let _ = CancelIoEx(h, None);
        }
        let _ = writer.join();
        close(h);
        self.push(hwnd, Evt::Gone { id, why });
    }
}

fn close(h: HANDLE) {
    unsafe {
        let _ = CloseHandle(h);
    }
}

/// Wait until something can be read, the pipe breaks or `deadline` passes.
fn wait_for_data(h: HANDLE, deadline: Instant) -> bool {
    loop {
        let mut avail = 0u32;
        if unsafe { PeekNamedPipe(h, None, 0, None, Some(&mut avail), None) }.is_err() {
            return false;
        }
        if avail > 0 {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run one overlapped operation to its end. `start` begins it; the result is the byte count, or None if it failed or
/// was cancelled (`CancelIoEx`). The pipe is overlapped because a plain handle would let a waiting read block every
/// write to the same pipe, and the writer thread must be able to send while the reader waits.
fn overlapped(h: HANDLE, start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>) -> Option<usize> {
    unsafe {
        let ev = CreateEventW(None, true, false, None).ok()?;
        let mut ov = OVERLAPPED { hEvent: ev, ..Default::default() };
        let mut n = 0u32;
        let ok = match start(&mut ov) {
            Ok(()) => GetOverlappedResult(h, &ov, &mut n, true).is_ok(),
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => GetOverlappedResult(h, &ov, &mut n, true).is_ok(),
            Err(_) => false,
        };
        let _ = CloseHandle(ev);
        ok.then_some(n as usize)
    }
}

/// Wait for a client on a new instance of the pipe (one that connected already counts).
unsafe fn accept(h: HANDLE) {
    let mut failed = false;
    let ev = CreateEventW(None, true, false, None);
    if let Ok(ev) = ev {
        let mut ov = OVERLAPPED { hEvent: ev, ..Default::default() };
        match ConnectNamedPipe(h, Some(&mut ov)) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                let mut n = 0u32;
                let _ = GetOverlappedResult(h, &ov, &mut n, true);
            }
            Err(e) => failed = e.code() != ERROR_PIPE_CONNECTED.to_hresult(),
        }
        let _ = CloseHandle(ev);
    } else {
        failed = true;
    }
    if failed {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn read_some(h: HANDLE, buf: &mut [u8]) -> Option<usize> {
    overlapped(h, |ov| unsafe { ReadFile(h, Some(&mut *buf), None, Some(ov)) }).filter(|&n| n > 0)
}

fn write_all(h: HANDLE, bytes: &[u8]) -> bool {
    let mut off = 0;
    while off < bytes.len() {
        match overlapped(h, |ov| unsafe { WriteFile(h, Some(&bytes[off..]), None, Some(ov)) }) {
            Some(n) if n > 0 => off += n,
            _ => return false,
        }
    }
    true
}

unsafe fn client_pid(h: HANDLE) -> Option<u32> {
    let mut pid = 0u32;
    GetNamedPipeClientProcessId(h, &mut pid).ok()?;
    Some(pid)
}

/// The full path of the program a process runs.
fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(p, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(p);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// The current user's SID as text (`S-1-5-21-...`).
unsafe fn user_sid() -> Option<String> {
    let mut token = HANDLE::default();
    OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
    let mut len = 0u32;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
    // u64 units: TOKEN_USER holds pointers and wants 8-byte alignment
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    let ok = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len).is_ok();
    let _ = CloseHandle(token);
    if !ok {
        return None;
    }
    let tu = &*(buf.as_ptr() as *const TOKEN_USER);
    let mut s = PWSTR::null();
    ConvertSidToStringSidW(tu.User.Sid, &mut s).ok()?;
    let out = s.to_string().ok();
    let _ = LocalFree(HLOCAL(s.0 as *mut c_void));
    out
}

/// A security descriptor that gives the current user, and nobody else, full access (`D:P` = no inherited entries).
/// Kept for as long as the process lives: every instance of the pipe is made with it.
unsafe fn user_only_descriptor() -> Option<isize> {
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", user_sid()?));
    let mut sd = PSECURITY_DESCRIPTOR::default();
    ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None).ok()?;
    Some(sd.0 as isize)
}

unsafe fn create_pipe(name: &str, first: bool, sd: isize) -> Option<HANDLE> {
    let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd as *mut c_void, bInheritHandle: false.into() };
    let open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { FILE_FLAGS_AND_ATTRIBUTES(0) };
    let h = CreateNamedPipeW(
        &HSTRING::from(name),
        open,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_UNLIMITED_INSTANCES,
        64 * 1024,
        64 * 1024,
        0,
        Some(&sa),
    );
    (h != INVALID_HANDLE_VALUE).then_some(h)
}

/// `\\.\pipe\hesewake-notch-ext-<user SID>`
pub(crate) fn pipe_name() -> Option<String> {
    unsafe { user_sid() }.map(|s| format!(r"\\.\pipe\hesewake-notch-ext-{s}"))
}

static HOST: std::sync::OnceLock<Arc<Host>> = std::sync::OnceLock::new();

/// The host of this process.
pub(crate) fn host() -> &'static Arc<Host> {
    HOST.get_or_init(|| Host::new(env!("CARGO_PKG_VERSION")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING};

    static N: AtomicU64 = AtomicU64::new(0);

    /// A host on a pipe of its own (the real pipe may be in use by a running notch). The test program stands in for the
    /// extension's exe: its image path is what the host sees for the client.
    fn host_with(allowed: &[(&str, &str, bool)], hello_wait: Duration) -> (Arc<Host>, String) {
        let name = format!(r"\\.\pipe\hesewake-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
        let h = Host::with_hello_wait("9.9.9", hello_wait);
        h.set_allowed(allowed.iter().map(|(id, exe, on)| (id.to_string(), exe.to_string(), *on)).collect());
        h.serve(&name, 0).expect("pipe");
        (h, name)
    }

    fn me() -> String {
        std::env::current_exe().unwrap().to_string_lossy().into_owned()
    }

    fn connect(name: &str) -> HANDLE {
        for _ in 0..100 {
            if let Ok(h) = unsafe { CreateFileW(&HSTRING::from(name), 0xC000_0000, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) } {
                return h;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("could not connect to {name}");
    }

    /// The client's end is an ordinary (synchronous) handle; it never reads and writes at the same time.
    fn sync_write(h: HANDLE, bytes: &[u8]) {
        let mut n = 0u32;
        unsafe { WriteFile(h, Some(bytes), Some(&mut n), None) }.expect("write");
        assert_eq!(n as usize, bytes.len());
    }

    fn sync_read(h: HANDLE, buf: &mut [u8]) -> Option<usize> {
        let mut n = 0u32;
        unsafe { ReadFile(h, Some(buf), Some(&mut n), None) }.ok()?;
        (n > 0).then_some(n as usize)
    }

    fn put(h: HANDLE, v: serde_json::Value) {
        sync_write(h, &proto::frame(&v).unwrap());
    }

    /// The next frame from the host, None if the pipe closed or nothing came within 3 seconds.
    fn get(h: HANDLE, fb: &mut proto::FrameBuf) -> Option<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut buf = vec![0u8; 4096];
        loop {
            if let Ok(Some(p)) = fb.next() {
                return serde_json::from_slice(&p).ok();
            }
            if !wait_for_data(h, deadline) {
                return None;
            }
            let n = sync_read(h, &mut buf)?;
            fb.push(&buf[..n]);
        }
    }

    fn hello(id: &str, ver: u32) -> serde_json::Value {
        json!({"t": "hello", "proto": ver, "id": id, "name": "Test", "version": "1"})
    }

    fn events_until(host: &Host, want: impl Fn(&[Evt]) -> bool) -> Vec<Evt> {
        let mut all = Vec::new();
        for _ in 0..150 {
            all.extend(host.take_events());
            if want(&all) {
                return all;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("events never arrived: {all:?}");
    }

    #[test]
    fn an_allowed_program_gets_a_welcome_and_its_messages_arrive() {
        let (host, name) = host_with(&[("test-ext", &me(), true)], HELLO_WAIT);
        host.set_info("id", [1, 2, 3]);
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(c, hello("test-ext", 1));
        let w = get(c, &mut fb).expect("welcome");
        assert_eq!((w["t"].as_str(), w["app"].as_str(), w["lang"].as_str(), w["accent"].clone()), (Some("welcome"), Some("9.9.9"), Some("id"), json!([1, 2, 3])));
        put(c, json!({"t": "slide", "rev": 1, "rows": [{"id": "1", "text": "One"}]}));
        put(c, json!({"t": "slide", "rev": 1, "rows": [{"id": "2", "text": "A repeat"}]}));
        put(c, json!({"t": "toast", "title": "Hi"}));
        put(c, json!({"t": "toast", "title": "Too soon"}));
        put(c, json!({"t": "status", "text": "Ready"}));
        let ev = events_until(&host, |e| e.iter().any(|x| matches!(x, Evt::Msg { msg: FromExt::Status(_), .. })));
        assert_eq!(ev[0], Evt::Connected { id: "test-ext".into() });
        let slides = ev.iter().filter(|e| matches!(e, Evt::Msg { msg: FromExt::Slide(_), .. })).count();
        let toasts = ev.iter().filter(|e| matches!(e, Evt::Msg { msg: FromExt::Toast { .. }, .. })).count();
        assert_eq!((slides, toasts), (1, 1), "a repeated rev and a toast right after another are dropped");
        assert!(host.connected("test-ext"));
        assert!(host.send("test-ext", proto::visible(true)));
        assert_eq!(get(c, &mut fb), Some(json!({"t": "visible", "on": true})));
        assert!(!host.send("nobody", proto::back()));
        close(c);
        events_until(&host, |e| e.contains(&Evt::Gone { id: "test-ext".into(), why: "closed" }));
        assert!(!host.connected("test-ext"));
    }

    #[test]
    fn strangers_and_the_wrong_program_are_turned_away() {
        let (_host, name) = host_with(&[("test-ext", &me(), true), ("off-ext", &me(), false), ("other-ext", r"C:\Elsewhere\other.exe", true)], HELLO_WAIT);
        for (id, ver, reason) in [("nobody", 1, "unknown"), ("off-ext", 1, "disabled"), ("other-ext", 1, "unknown"), ("test-ext", 2, "proto")] {
            let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
            put(c, hello(id, ver));
            assert_eq!(get(c, &mut fb), Some(json!({"t": "reject", "reason": reason})), "{id}");
            assert_eq!(get(c, &mut fb), None, "then the pipe is closed");
            close(c);
        }
    }

    #[test]
    fn one_connection_per_extension() {
        let (host, name) = host_with(&[("test-ext", &me(), true)], HELLO_WAIT);
        let (a, mut fa) = (connect(&name), proto::FrameBuf::default());
        put(a, hello("test-ext", 1));
        assert_eq!(get(a, &mut fa).unwrap()["t"], "welcome");
        let (b, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(b, hello("test-ext", 1));
        assert_eq!(get(b, &mut fb), Some(json!({"t": "reject", "reason": "busy"})));
        assert!(host.connected("test-ext"), "the first one is not disturbed");
        close(a);
        close(b);
    }

    #[test]
    fn broken_data_closes_the_connection() {
        let (host, name) = host_with(&[("test-ext", &me(), true)], HELLO_WAIT);
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(c, hello("test-ext", 1));
        get(c, &mut fb).expect("welcome");
        for _ in 0..BAD_IN_A_ROW {
            sync_write(c, &[3, 0, 0, 0, b'n', b'o', b'!']);
        }
        events_until(&host, |e| e.contains(&Evt::Gone { id: "test-ext".into(), why: "bad data" }));
        close(c);
        // an announced length over the limit ends it at once
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(c, hello("test-ext", 1));
        get(c, &mut fb).expect("welcome again: the first one is gone");
        sync_write(c, &((proto::MAX_FRAME + 1) as u32).to_le_bytes());
        events_until(&host, |e| e.iter().filter(|x| matches!(x, Evt::Gone { why: "bad data", .. })).count() >= 1);
        close(c);
    }

    #[test]
    fn silence_and_a_wrong_first_message_do_not_hold_a_connection() {
        let (_host, name) = host_with(&[("test-ext", &me(), true)], Duration::from_millis(300));
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        assert_eq!(get(c, &mut fb), None, "nothing said: closed after the wait");
        close(c);
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(c, json!({"t": "slide", "rev": 1}));
        assert_eq!(get(c, &mut fb), None, "the first message must be hello");
        close(c);
    }

    #[test]
    fn disabling_or_disconnecting_sends_quit_and_ends_it() {
        let (host, name) = host_with(&[("test-ext", &me(), true)], HELLO_WAIT);
        let (c, mut fb) = (connect(&name), proto::FrameBuf::default());
        put(c, hello("test-ext", 1));
        get(c, &mut fb).expect("welcome");
        host.set_allowed(vec![("test-ext".into(), me(), false)]);
        assert_eq!(get(c, &mut fb), Some(json!({"t": "quit"})));
        events_until(&host, |e| e.iter().any(|x| matches!(x, Evt::Gone { id, .. } if id == "test-ext")));
        assert!(!host.connected("test-ext"));
        close(c);
    }

    /// Run by hand once the example is built (`cargo build --release` in examples/hello-extension):
    /// `cargo test --release the_example_extension -- --ignored`. A real program, started like the notch starts one,
    /// talking over a real pipe to the host.
    #[test]
    #[ignore = "needs examples/hello-extension built"]
    fn the_example_extension_talks_to_the_host() {
        let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"examples\hello-extension\target\release\hello-extension.exe");
        assert!(exe.exists(), "build the example first");
        let exe = exe.to_string_lossy().into_owned();
        let (host, name) = host_with(&[("hello-extension", &exe, true)], HELLO_WAIT);
        let mut child = std::process::Command::new(&exe).env("HESEWAKE_NOTCH_PIPE", &name).spawn().unwrap();
        let wait = |what: &str, want: &dyn Fn(&FromExt) -> bool| -> Vec<FromExt> {
            let mut got = Vec::new();
            for _ in 0..250 {
                got.extend(host.take_events().into_iter().filter_map(|e| match e {
                    Evt::Msg { msg, .. } => Some(msg),
                    _ => None,
                }));
                if got.iter().any(want) {
                    return got;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("{what}: nothing came, got {got:?}");
        };
        // a person clicks slower than the host's limit of messages per second, and the example answers each click with two
        let send = |frame: Vec<u8>| {
            std::thread::sleep(Duration::from_millis(400));
            host.send("hello-extension", frame)
        };
        let home = wait("home page", &|m| matches!(m, FromExt::Slide(_)));
        let FromExt::Slide(s) = home.iter().find(|m| matches!(m, FromExt::Slide(_))).unwrap() else { unreachable!() };
        assert_eq!((s.title.as_str(), s.tabs.len(), s.actions.len(), s.sections[0].rows.len()), ("Hel|lo", 3, 2, 12));
        assert!(send(proto::tab("odd")));
        let odd = wait("odd tab", &|m| matches!(m, FromExt::Slide(s) if s.tab == "odd"));
        assert!(odd.iter().any(|m| matches!(m, FromExt::Slide(s) if s.sections[0].rows.len() == 6)));
        assert!(send(proto::row("3")));
        let d = wait("detail", &|m| matches!(m, FromExt::Detail(_)));
        assert!(d.iter().any(|m| matches!(m, FromExt::Detail(d) if d.for_id == "3" && d.buttons.len() == 3 && d.buttons[0].link.is_some() && d.buttons[1].copy.as_deref() == Some("Item 3"))));
        assert!(send(proto::action("report")));
        wait("report page", &|m| matches!(m, FromExt::Slide(s) if s.back && s.stepper.as_ref().is_some_and(|st| st.label == "today" && st.prev && !st.next)));
        assert!(send(proto::step(-1)));
        wait("yesterday", &|m| matches!(m, FromExt::Slide(s) if s.stepper.as_ref().is_some_and(|st| st.label == "yesterday" && st.next)));
        assert!(send(proto::action("notify")));
        wait("toast and badge", &|m| matches!(m, FromExt::Badge(true)));
        assert!(send(proto::visible(true)));
        wait("badge off", &|m| matches!(m, FromExt::Badge(false)));
        // turning it off: the host sends quit, the program says bye and ends
        host.set_allowed(vec![("hello-extension".into(), exe.clone(), false)]);
        wait("bye", &|m| matches!(m, FromExt::Bye));
        for _ in 0..100 {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        panic!("the example did not end after quit");
    }

    #[test]
    fn the_real_pipe_name_has_the_user_sid() {
        let n = pipe_name().expect("a SID");
        assert!(n.starts_with(r"\\.\pipe\hesewake-notch-ext-S-1-5-"), "{n}");
    }
}

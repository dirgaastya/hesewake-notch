//! Notifikasi Windows (UserNotificationListener): akses, pembacaan, dan event ke UI thread.
//! Daftar toast dibaca berkala (polling ringan) karena event `NotificationChanged` tidak tersedia tanpa package
//! identity. Event dikirim ke UI lewat slot bersama + `WM_NOTIF` (pola yang sama dengan bt.rs).

use super::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::ApplicationModel::AppInfo;
use windows::Foundation::{AsyncOperationCompletedHandler, Size};
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};

/// Pesan ke window: ada event notifikasi baru di slot.
pub(crate) const WM_NOTIF: u32 = WM_APP + 3;

const MAX_ITEMS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Access {
    Unknown,
    Allowed,
    Denied,
}

pub(crate) enum NotifEvent {
    Added { id: u32, app: String, title: String, body: String, at_unix: u64, icon: Option<Vec<u8>>, avatar: Option<Vec<u8>>, initial: bool },
    Removed(u32),
    Status(Access),
}

pub(crate) type NotifSlot = Arc<Mutex<Vec<NotifEvent>>>;

pub(crate) struct NotifItem {
    pub(crate) id: u32,
    pub(crate) app: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) at_unix: u64,
    pub(crate) unread: bool,
    pub(crate) icon: Option<Pixmap>,
    /// Foto profil kontak (dari payload toast) bila ada; digambar bulat dengan logo aplikasi sebagai lencana.
    pub(crate) avatar: Option<Pixmap>,
    /// Mulai animasi hapus (None = tampil normal); item dibuang setelah animasi selesai.
    pub(crate) dismiss: Option<Instant>,
}

const ICON_PX: u32 = 32;

// Listener harus hidup selama app jalan agar event tetap datang.
static LISTENER: Mutex<Option<UserNotificationListener>> = Mutex::new(None);

fn push(slot: &NotifSlot, hwnd: isize, ev: NotifEvent) {
    slot.lock().unwrap_or_else(|p| p.into_inner()).push(ev);
    unsafe {
        let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_NOTIF, WPARAM(0), LPARAM(0));
    }
}

/// Mulai (di UI thread): cek izin, minta bila belum ditentukan, lalu pasang listener.
pub(crate) fn start(hwnd: isize, slot: NotifSlot) {
    let l = match UserNotificationListener::Current() {
        Ok(l) => l,
        Err(e) => {
            perf::note(&format!("notif: Current() gagal: {e}"));
            push(&slot, hwnd, NotifEvent::Status(Access::Denied));
            return;
        }
    };
    match l.GetAccessStatus() {
        Ok(UserNotificationListenerAccessStatus::Allowed) => begin(hwnd, slot),
        Ok(UserNotificationListenerAccessStatus::Unspecified) => match l.RequestAccessAsync() {
            Ok(op) => {
                let slot2 = slot.clone();
                let h = AsyncOperationCompletedHandler::new(move |op, _| {
                    let allowed = op.as_ref().and_then(|o| o.GetResults().ok()) == Some(UserNotificationListenerAccessStatus::Allowed);
                    if allowed {
                        begin(hwnd, slot2.clone());
                    } else {
                        push(&slot2, hwnd, NotifEvent::Status(Access::Denied));
                    }
                    Ok(())
                });
                if op.SetCompleted(&h).is_err() {
                    push(&slot, hwnd, NotifEvent::Status(Access::Denied));
                }
            }
            Err(_) => push(&slot, hwnd, NotifEvent::Status(Access::Denied)),
        },
        _ => push(&slot, hwnd, NotifEvent::Status(Access::Denied)),
    }
}

/// Poll berjalan hanya bila fitur aktif dan layar menyala.
static ENABLED: AtomicBool = AtomicBool::new(true);
static AVATAR: AtomicBool = AtomicBool::new(true);
static PAUSED: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Ambil foto profil dari payload toast (Settings > Notifications > Contact photos).
pub(crate) fn set_avatar(on: bool) {
    AVATAR.store(on, Ordering::Relaxed);
}

pub(crate) fn avatar_enabled() -> bool {
    AVATAR.load(Ordering::Relaxed)
}

pub(crate) fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
}

const POLL_MS: u64 = 1500;

/// `NotificationChanged` tidak bisa didaftarkan oleh exe tanpa package identity (E_ELEMENTNOTFOUND), sedangkan
/// `GetNotificationsAsync` bisa. Jadi daftar toast dibaca berkala di thread terpisah dan dibandingkan dengan keadaan
/// sebelumnya (id baru = Added, id hilang = Removed). Panggilannya murah; berhenti saat layar mati / fitur dimatikan.
fn begin(hwnd: isize, slot: NotifSlot) {
    push(&slot, hwnd, NotifEvent::Status(Access::Allowed));
    notif_img::start_watchers();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        // Objek listener dibuat di thread ini (MTA): objek dari UI thread (STA) tidak boleh dipakai lintas thread.
        let Ok(l) = UserNotificationListener::Current() else {
            perf::note("notif: Current() gagal di thread poll");
            return;
        };
        *LISTENER.lock().unwrap_or_else(|p| p.into_inner()) = Some(l.clone());
        perf::note("notif: thread poll mulai");
        let mut known: HashSet<u32> = HashSet::new();
        let mut first = true; // isi awal: sudah ada di Action Center, ditandai dibaca tanpa toast
        loop {
            if ENABLED.load(Ordering::Relaxed) && !PAUSED.load(Ordering::Relaxed) {
                perf::note("notif: poll -> GetNotificationsAsync");
                let res = l.GetNotificationsAsync(NotificationKinds::Toast).and_then(|a| a.get());
                perf::note(&format!("notif: poll selesai ok={} mem={:.1}MB", res.is_ok(), perf::mem_mb()));
                if let Err(e) = &res {
                    perf::note(&format!("notif: poll gagal: {e}"));
                }
                if let Ok(list) = res {
                    let mut now_ids: HashSet<u32> = HashSet::new();
                    for n in list {
                        let Ok(id) = n.Id() else { continue };
                        now_ids.insert(id);
                        if !known.contains(&id) {
                            let ev = convert(&n, first);
                            if !first {
                                perf::note(&format!("notif: id baru {id}, convert={}", ev.is_some()));
                            }
                            if let Some(ev) = ev {
                                push(&slot, hwnd, ev);
                            }
                        }
                    }
                    for id in known.difference(&now_ids) {
                        push(&slot, hwnd, NotifEvent::Removed(*id));
                    }
                    if first {
                        perf::note(&format!("notif: awal {} item", now_ids.len()));
                    }
                    known = now_ids;
                    first = false;
                }
            }
            std::thread::sleep(Duration::from_millis(POLL_MS));
        }
    });
}

/// Hapus notifikasi dari Windows (Action Center) juga. Dijalankan di thread terpisah.
pub(crate) fn remove_from_windows(ids: Vec<u32>) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let l = LISTENER.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(l) = l {
            for id in ids {
                let _ = l.RemoveNotification(id);
            }
        }
    });
}

/// Pisahkan elemen teks toast menjadi (judul, isi): elemen pertama judul, sisanya digabung per baris.
pub(crate) fn split_texts(texts: &[String]) -> (String, String) {
    let t: Vec<&str> = texts.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    match t.split_first() {
        Some((first, rest)) => (first.to_string(), rest.join("\n")),
        None => (String::new(), String::new()),
    }
}

fn convert(n: &UserNotification, initial: bool) -> Option<NotifEvent> {
    let id = n.Id().ok()?;
    let info = n.AppInfo().ok();
    let display = info.as_ref().and_then(|a: &AppInfo| a.DisplayInfo().ok());
    let app = display.as_ref().and_then(|d| d.DisplayName().ok()).map(|s| s.to_string()).unwrap_or_default();

    let binding = n.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let mut texts: Vec<String> = Vec::new();
    for t in binding.GetTextElements().ok()? {
        if let Ok(s) = t.Text() {
            texts.push(s.to_string());
        }
    }
    let (title, body) = split_texts(&texts);
    if title.is_empty() && body.is_empty() {
        return None;
    }
    // DateTime WinRT = satuan 100 ns sejak 1601-01-01.
    let at_unix = n.CreationTime().ok().map(|t| (t.UniversalTime / 10_000_000 - 11_644_473_600).max(0) as u64).unwrap_or(0);
    let t0 = Instant::now();
    let icon = display.as_ref().and_then(|d| logo_with_timeout(d.clone()));
    perf::note(&format!("notif: convert id={id} app='{app}' icon={} ({} ms) mem={:.1}MB", icon.is_some(), t0.elapsed().as_millis(), perf::mem_mb()));
    let avatar = if AVATAR.load(Ordering::Relaxed) { notif_img::avatar_rgba(id) } else { None };
    Some(NotifEvent::Added { id, app, title, body, at_unix, icon, avatar, initial })
}

/// Baca logo di thread sendiri dengan batas waktu: beberapa logo aplikasi tidak pernah selesai dimuat dan tidak
/// boleh menahan polling.
fn logo_with_timeout(d: windows::ApplicationModel::AppDisplayInfo) -> Option<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let _ = tx.send(logo_rgba(&d));
    });
    rx.recv_timeout(Duration::from_millis(1500)).ok().flatten()
}

fn logo_rgba(d: &windows::ApplicationModel::AppDisplayInfo) -> Option<Vec<u8>> {
    use windows::Storage::Streams::DataReader;
    let r = d.GetLogo(Size { Width: ICON_PX as f32, Height: ICON_PX as f32 }).ok()?;
    let s = r.OpenReadAsync().ok()?.get().ok()?;
    let size = s.Size().ok()? as u32;
    if size == 0 || size > 2_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&s).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let img = decode_limited(&buf)?;
    Some(img.resize_to_fill(ICON_PX, ICON_PX, image::imageops::FilterType::Triangle).to_rgba8().into_raw())
}

/// `--demo-notifs`: daftar notifikasi palsu untuk uji visual slide Notifications (tanpa membaca notifikasi asli).
pub(crate) fn demo_items() -> Vec<NotifItem> {
    let now = now_unix();
    let mk = |id: u32, app: &str, title: &str, body: &str, ago: u64, unread: bool| NotifItem {
        id,
        app: app.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        at_unix: now.saturating_sub(ago),
        unread,
        icon: None,
        avatar: None,
        dismiss: None,
    };
    vec![
        mk(1, "Screenshot", "Lightshot", "Your screenshot is copied to clipboard", 120, false),
        mk(2, "Team Chat", "Tim Proyek", "Rina \u{1F4CB} Laporan Task Total task: 59 Detail rapat besok pagi jam sembilan", 720, true),
        mk(3, "Chat", "Budi Santoso: Rapat \u{2713} 日本語", "oke, kita cek di bulan yg bru ntar ya \u{201c}siap\u{201d} \u{2014} terima kasih", 960, true),
    ]
}

/// Waktu relatif singkat: "now", "5m", "3h", "2d".
pub(crate) fn relative_time(at_unix: u64, now_unix: u64) -> String {
    let d = now_unix.saturating_sub(at_unix);
    match d {
        0..=59 => tr("now").to_string(),
        60..=3599 => format!("{}m", d / 60),
        3600..=86_399 => fill(tr("{}h"), &[&(d / 3600)]),
        _ => fill(tr("{}d"), &[&(d / 86_400)]),
    }
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Gambar ikon aplikasi (pixmap kecil) membulat; tanpa ikon: lingkaran dengan huruf pertama nama aplikasi.
pub(crate) fn draw_app_icon(g: &mut Gfx, item: &NotifItem, x: f32, y: f32, size: f32, al: f32) {
    if let Some(pm) = &item.icon {
        let s = size / pm.width() as f32;
        let mut p = Paint::default();
        p.shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, al, Transform::from_row(s, 0.0, 0.0, s, x, y));
        p.anti_alias = true;
        g.fill(&rrect(x, y, size, size, size * 0.24), &p);
    } else {
        g.fill(&rrect(x, y, size, size, size * 0.24), &paint(60, 64, 78, al));
        let ch: String = item.app.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
        g.txt_c(&ch, x + size / 2.0, y + ty(size, size * 0.5), size * 0.5, [230, 232, 240], al, size, true);
    }
}

/// Avatar bulat (foto kontak) + logo aplikasi kecil sebagai lencana; tanpa avatar jatuh ke ikon aplikasi biasa.
pub(crate) fn draw_avatar(g: &mut Gfx, item: &NotifItem, x: f32, y: f32, size: f32, al: f32) {
    let Some(pm) = &item.avatar else {
        draw_app_icon(g, item, x, y, size, al);
        return;
    };
    let s = size / pm.width() as f32;
    let mut p = Paint::default();
    p.shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, al, Transform::from_row(s, 0.0, 0.0, s, x, y));
    p.anti_alias = true;
    g.fill(&rrect(x, y, size, size, size / 2.0), &p);
    if item.icon.is_some() {
        let b = size * 0.46;
        let (bx, by) = (x + size - b + size * 0.07, y + size - b + size * 0.07);
        g.circle(bx + b / 2.0, by + b / 2.0, b / 2.0 + 1.6, &paint(6, 8, 14, al));
        draw_app_icon(g, item, bx, by, b, al);
    }
}

impl App {
    pub(crate) fn unread_count(&self) -> usize {
        self.notifs.iter().filter(|n| n.unread && n.dismiss.is_none()).count()
    }

    /// Terapkan event dari thread listener (di UI thread).
    pub(crate) fn notif_apply(&mut self, events: Vec<NotifEvent>) {
        for ev in events {
            match ev {
                NotifEvent::Status(a) => self.notif_access = a,
                NotifEvent::Removed(id) => self.notifs.retain(|n| n.id != id),
                NotifEvent::Added { id, app, title, body, at_unix, icon, avatar, initial } => {
                    if !self.notif_on || self.notifs.iter().any(|n| n.id == id) || notif_muted(&app) {
                        continue;
                    }
                    let icon = icon.and_then(|rgba| pixmap_from_rgba(&rgba, ICON_PX));
                    let avatar = avatar.and_then(|rgba| pixmap_from_rgba(&rgba, notif_img::AVATAR_PX));
                    self.notifs.push(NotifItem { id, app, title, body, at_unix, unread: !initial, icon, avatar, dismiss: None });
                    self.notifs.sort_by(|a, b| b.at_unix.cmp(&a.at_unix));
                    self.notifs.truncate(MAX_ITEMS);
                    if !initial {
                        self.bell_t = Instant::now();
                        // Layar penuh / presentasi: badge tetap naik, toast ditahan.
                        if !sys::user_fullscreen() {
                            self.push_toast(toast::ToastKind::Notification { id });
                        }
                    }
                }
            }
        }
    }

    /// Posisi lonceng di notch kecil (koordinat kanvas), bila tampil.
    pub(crate) fn bell_pos(&self) -> Option<(f32, f32)> {
        if !self.notif_on || self.notif_access != Access::Allowed || self.expanded() || self.toast.is_some() {
            return None;
        }
        if self.media.playing {
            Some((CX + self.w / 2.0 - 58.0 - self.priv_extra(), self.h / 2.0))
        } else if self.unread_count() > 0 && !self.thin() {
            Some((CX + self.w / 2.0 - 20.0 - self.priv_extra(), self.h / 2.0))
        } else {
            None
        }
    }

    /// Ruang tambahan (px) yang dibutuhkan notch kecil saat lonceng tampil bersama lagu.
    pub(crate) fn bell_extra(&self) -> f32 {
        if self.notif_on && self.notif_access == Access::Allowed && self.media.playing { 24.0 } else { 0.0 }
    }

    /// Goyang lonceng 0.9 dtk setelah notifikasi baru.
    pub(crate) fn bell_animating(&self) -> bool {
        self.notif_on && !self.expanded() && self.bell_t.elapsed().as_secs_f32() < 0.9
    }

    pub(crate) fn draw_bell(&self, g: &mut Gfx, al: f32) {
        let Some((bx, by)) = self.bell_pos() else { return };
        let unread = self.unread_count();
        let e = self.bell_t.elapsed().as_secs_f32();
        let swing = if unread > 0 { (e * 22.0).sin() * (1.0 - e / 0.9).max(0.0) } else { 0.0 };
        let a = if unread > 0 { al } else { al * 0.45 };
        g.icon_bell(bx, by, 14.0, swing, &paint(255, 255, 255, a));
        if unread > 0 {
            let pop = (1.0 - (-9.0 * e).exp() * (16.0 * e).cos()).clamp(0.0, 1.2);
            let r = 5.5 * pop.min(1.15);
            let (cx, cy) = (bx + 6.0, by - 5.5);
            g.circle(cx, cy, r, &paint(ACCENT[0], ACCENT[1], ACCENT[2], al));
            let label = if unread > 9 { "9+".to_string() } else { unread.to_string() };
            if r > 4.0 {
                g.txt_c(&label, cx, cy - 4.5, 8.5, [255, 255, 255], al, 12.0, true);
            }
        }
    }
}

/// Aplikasi yang dibisukan: satu nama per baris di `notif_mute.txt` (tanpa membedakan huruf besar/kecil).
fn notif_muted(app: &str) -> bool {
    std::fs::read_to_string(data_path("notif_mute.txt"))
        .map(|s| s.lines().any(|l| !l.trim().is_empty() && l.trim().eq_ignore_ascii_case(app)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_title_and_body() {
        let t = vec!["Alice".to_string(), "  ".to_string(), "Hello".to_string(), "second line".to_string()];
        assert_eq!(split_texts(&t), ("Alice".to_string(), "Hello\nsecond line".to_string()));
        assert_eq!(split_texts(&[]), (String::new(), String::new()));
        assert_eq!(split_texts(&["Only title".to_string()]), ("Only title".to_string(), String::new()));
    }

    #[test]
    fn formats_relative_time() {
        assert_eq!(relative_time(1000, 1010), "now");
        assert_eq!(relative_time(1000, 1000 + 300), "5m");
        assert_eq!(relative_time(0, 3 * 3600 + 5), "3h");
        assert_eq!(relative_time(0, 2 * 86_400), "2d");
        assert_eq!(relative_time(2000, 1000), "now");
    }
}

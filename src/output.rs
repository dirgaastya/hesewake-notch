//! Audio output switcher (Music slide): the active playback devices, and switching the Windows default device.
//! Windows has no public API for that; like EarTrumpet and SoundSwitch this uses the undocumented
//! `IPolicyConfig::SetDefaultEndpoint`, for all three roles so meeting apps follow too. If it fails, the Windows
//! sound settings page opens instead.

use super::*;
use windows::core::{GUID, HSTRING, PCWSTR as WPCWSTR, PWSTR};
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    PKEY_AudioEndpoint_FormFactor,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

/// Message to the window: the playback devices or the default device changed (sent from audio.rs).
pub(crate) const WM_OUTPUT: u32 = WM_APP + 15;

/// An active playback device.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Device {
    pub(crate) id: String,
    /// Windows friendly name, e.g. "Speakers (Realtek(R) Audio)".
    pub(crate) name: String,
    /// Headphones or a headset (by the endpoint form factor): drawn with a headphone icon.
    pub(crate) phones: bool,
}

/// The three default-device roles Windows keeps. Apps pick one: most use Console/Multimedia, calls use Communications.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Role {
    Console,
    Multimedia,
    Communications,
}

pub(crate) const ROLES: [Role; 3] = [Role::Console, Role::Multimedia, Role::Communications];

/// Where the device list comes from: Windows, or a fake in tests.
pub(crate) trait Endpoints {
    /// Active playback devices, in Windows' order.
    fn devices(&self) -> Vec<Device>;
    /// Id of the default playback device.
    fn current(&self) -> Option<String>;
    /// Make `id` the default for `role`. False if Windows refused.
    fn set_role(&self, id: &str, role: Role) -> bool;
}

/// Make `id` the default for every role (each role is tried even if an earlier one failed). False if any failed.
pub(crate) fn switch(ep: &dyn Endpoints, id: &str) -> bool {
    ROLES.iter().fold(true, |ok, &r| ep.set_role(id, r) && ok)
}

/// "Speakers (Realtek(R) Audio)" -> ("Speakers", "Realtek(R) Audio"); a name without a trailing group stays whole.
pub(crate) fn split_name(name: &str) -> (&str, &str) {
    let Some(body) = name.strip_suffix(')') else { return (name, "") };
    let mut depth = 0;
    for (i, c) in body.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' if depth > 0 => depth -= 1,
            '(' => {
                let head = body[..i].trim_end();
                return if head.is_empty() { (name, "") } else { (head, &body[i + 1..]) };
            }
            _ => {}
        }
    }
    (name, "")
}

/// The cached device list and the current default.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Outputs {
    pub(crate) list: Vec<Device>,
    pub(crate) cur: Option<String>,
}

impl Outputs {
    pub(crate) fn read(ep: &dyn Endpoints) -> Outputs {
        let list = ep.devices();
        let cur = if list.is_empty() { None } else { ep.current() };
        Outputs { list, cur }
    }

    pub(crate) fn current(&self) -> Option<&Device> {
        self.list.iter().find(|d| Some(&d.id) == self.cur.as_ref())
    }

    fn index(&self) -> Option<usize> {
        self.list.iter().position(|d| Some(&d.id) == self.cur.as_ref())
    }

    /// The device `dir` steps after the current one (wrapping); None with fewer than two devices.
    pub(crate) fn next(&self, dir: i32) -> Option<&Device> {
        let n = self.list.len();
        if n < 2 {
            return None;
        }
        let i = match self.index() {
            Some(i) => (i as i32 + dir).rem_euclid(n as i32) as usize,
            None => 0,
        };
        self.list.get(i)
    }
}

// ---------------------------------------------------------------- layout (Music slide)

/// The output button, left of "previous" (the lyrics button sits at the same distance on the right).
pub(crate) const BTN: (f32, f32) = (MCX - 92.0, MCY);
const BTN_R: f32 = 14.0;
/// The device list covers the artwork, title and controls; the clock stays visible.
pub(crate) const PANEL: (f32, f32, f32, f32) = (LX - 6.0, 36.0, MX + MW + 2.0 - (LX - 6.0), 116.0);
pub(crate) const ROW_H: f32 = 28.0;
pub(crate) const MAX_ROWS: usize = 4;
/// One wheel notch (Windows delta): trackpads send smaller steps that add up to this.
const NOTCH: i32 = 120;

/// Top-left of visible row `i` in the list.
pub(crate) fn row_pos(i: usize) -> (f32, f32) {
    (PANEL.0, PANEL.1 + 2.0 + i as f32 * ROW_H)
}

pub(crate) fn on_button(x: f32, y: f32) -> bool {
    ((x - BTN.0).powi(2) + (y - BTN.1).powi(2)).sqrt() < BTN_R
}

pub(crate) fn in_panel(x: f32, y: f32) -> bool {
    in_rect(x, y, PANEL.0, PANEL.1, PANEL.2, PANEL.3)
}

/// The volume and "switched to" overlays over the controls row: wide enough to cover the output and lyrics buttons.
pub(crate) const OVERLAY: (f32, f32, f32, f32) = (MCX - 108.0, 110.0, 216.0, 36.0);
/// Seconds the "switched to" overlay stays, like the volume overlay.

#[cfg(not(test))]
fn backend() -> Sys {
    Sys
}

#[cfg(test)]
fn backend() -> Fake {
    Fake::default()
}

#[cfg(test)]
thread_local! {
    static FAKE: std::cell::RefCell<Fake> = std::cell::RefCell::new(Fake::default());
}

/// Test double: a device list, the default device, every `set_role` call, and whether Windows refuses.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct Fake {
    pub(crate) list: Vec<Device>,
    pub(crate) cur: Option<String>,
    pub(crate) calls: Vec<(String, Role)>,
    pub(crate) refuse: bool,
}

#[cfg(test)]
impl Endpoints for Fake {
    fn devices(&self) -> Vec<Device> {
        FAKE.with(|f| f.borrow().list.clone())
    }
    fn current(&self) -> Option<String> {
        FAKE.with(|f| f.borrow().cur.clone())
    }
    fn set_role(&self, id: &str, role: Role) -> bool {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.calls.push((id.to_string(), role));
            if f.refuse {
                return false;
            }
            if role == Role::Console {
                f.cur = Some(id.to_string());
            }
            true
        })
    }
}

#[cfg(test)]
pub(crate) fn set_fake(list: &[(&str, &str, bool)], cur: Option<&str>) {
    let list = list.iter().map(|&(id, name, phones)| Device { id: id.into(), name: name.into(), phones }).collect();
    FAKE.with(|f| *f.borrow_mut() = Fake { list, cur: cur.map(Into::into), ..Fake::default() });
}

#[cfg(test)]
pub(crate) fn with_fake<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with(|x| f(&mut x.borrow_mut()))
}

/// Ellipsized to `maxw` when it does not fit.
fn fit(g: &mut Gfx, s: &str, size: f32, bold: bool, maxw: f32) -> String {
    if g.width_ex(s, size, bold) <= maxw {
        return s.to_string();
    }
    match g.text.as_mut() {
        Some(t) => t.ellipsize(s, size, bold, maxw),
        None => s.to_string(),
    }
}

/// Device icon, then its name in bold and the driver part dimmer after it, within `w` from `x`.
fn draw_device(g: &mut Gfx, d: &Device, x: f32, cy: f32, w: f32, a: f32) {
    let p = paint(255, 255, 255, a);
    if d.phones {
        g.icon_headphone(x + 10.0, cy, 15.0, &p);
    } else {
        g.icon_speaker(x + 10.0, cy, 24.0, 2, false, &p); // its points are scaled by s/2: this matches the headphones
    }
    let (head, sub) = split_name(&d.name);
    let (tx, room) = (x + 26.0, w - 26.0);
    let head = fit(g, head, 13.0, true, room);
    g.txtb(&head, tx, cy - 8.5, 13.0, [255, 255, 255], a, room + 2.0);
    let left = room - g.width_ex(&head, 13.0, true) - 6.0;
    if !sub.is_empty() && left > 28.0 {
        let sub = fit(g, sub, 11.0, false, left);
        g.txt(&sub, tx + room - left, cy - 6.9, 11.0, TEXT_DIM, a, left + 2.0);
    }
}

impl App {
    /// Read the device list again (on WM_OUTPUT, and when the list opens).
    pub(crate) fn out_refresh(&mut self) {
        self.outs = Outputs::read(&backend());
        if self.outs.list.is_empty() {
            self.out_open = false;
        }
        self.out_scroll = self.out_scroll.min(self.outs.list.len().saturating_sub(MAX_ROWS));
    }

    /// Whether the device list is showing.
    pub(crate) fn out_list_shown(&self) -> bool {
        self.out_open && self.expanded() && self.is_slide(SlideId::Music) && !self.lyric_mode
    }

    /// Switch to device `id`. On success the overlay shows its name; if Windows refuses, the sound settings open.
    pub(crate) fn out_pick(&mut self, id: &str) {
        self.out_open = false;
        if self.outs.cur.as_deref() == Some(id) {
            return;
        }
        if switch(&backend(), id) {
            self.outs.cur = Some(id.to_string());
            self.out_touch();
        } else {
            sys::open_url("ms-settings:sound");
            self.out_refresh(); // some roles may have switched
        }
    }

    /// Device index under (x, y) in the open list.
    fn out_row_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..MAX_ROWS).find_map(|vi| {
            let (rx, ry) = row_pos(vi);
            let i = self.out_scroll + vi;
            (in_rect(x, y, rx, ry, PANEL.2, ROW_H) && i < self.outs.list.len()).then_some(i)
        })
    }

    /// Click on the Music slide. True if the output button or list took it.
    pub(crate) fn out_click(&mut self, x: f32, y: f32) -> bool {
        if self.lyric_mode {
            return false;
        }
        if self.out_list_shown() {
            if !on_button(x, y) && in_panel(x, y) {
                if let Some(i) = self.out_row_at(x, y) {
                    let id = self.outs.list[i].id.clone();
                    self.out_pick(&id);
                }
            } else {
                self.out_open = false; // the button again, or anywhere else: close without doing more
            }
            return true;
        }
        if !on_button(x, y) {
            return false;
        }
        self.out_refresh();
        if self.outs.list.is_empty() {
            sys::open_url("ms-settings:sound");
            return true;
        }
        self.out_open = true;
        // keep the current device in view
        let i = self.outs.index().unwrap_or(0);
        self.out_scroll = (i + 1).saturating_sub(MAX_ROWS);
        true
    }

    /// Wheel over the button (next/previous device) or over the open list (scroll). True if taken.
    pub(crate) fn out_wheel(&mut self, x: f32, y: f32, delta: i32) -> bool {
        if self.lyric_mode {
            return false;
        }
        if self.out_list_shown() && in_panel(x, y) {
            let max = self.outs.list.len().saturating_sub(MAX_ROWS);
            self.out_scroll = if delta < 0 { (self.out_scroll + 1).min(max) } else { self.out_scroll.saturating_sub(1) };
            return true;
        }
        if !on_button(x, y) {
            return false;
        }
        self.out_acc += delta;
        if self.out_acc.abs() < NOTCH {
            return true;
        }
        let dir = if self.out_acc < 0 { 1 } else { -1 }; // wheel down = next device
        self.out_acc = 0;
        match self.outs.next(dir).map(|d| d.id.clone()) {
            Some(id) => self.out_pick(&id),
            // only one device: show which one plays
            None if self.outs.current().is_some() => self.out_touch(),
            None => {}
        }
        true
    }

    /// Close the list (notch closed, slide changed).
    pub(crate) fn out_close(&mut self) {
        self.out_open = false;
    }

    /// Output button on the controls row: headphones or a speaker, after the current device.
    pub(crate) fn draw_out_button(&self, g: &mut Gfx, ox: f32, al: f32) {
        let (x, y) = (BTN.0 + ox, BTN.1);
        if self.out_open {
            g.circle(x, y, 14.0, &paint(255, 255, 255, 0.14 * al));
        }
        let p = paint(255, 255, 255, al * if self.outs.list.is_empty() { 0.4 } else { 0.9 });
        match self.outs.current() {
            Some(d) if d.phones => g.icon_headphone(x, y, 17.0, &p),
            _ => g.icon_speaker(x, y, 27.0, 2, false, &p),
        }
    }

    /// Overlay over the controls row after a switch: device icon and name.
    pub(crate) fn draw_out_overlay(&self, g: &mut Gfx, ox: f32, al: f32) {
        let a = al * music::overlay_alpha(self.out_t.elapsed().as_secs_f32());
        let Some(d) = self.outs.current() else { return };
        if a <= 0.01 {
            return;
        }
        let (x, y, w, h) = OVERLAY;
        let x = x + ox;
        g.fill(&rrect(x, y, w, h, 14.0), &paint(8, 10, 18, a));
        draw_device(g, d, x + 10.0, y + h / 2.0, w - 22.0, a);
    }

    /// The device list over the slide: the current device highlighted and ticked.
    pub(crate) fn draw_out_list(&self, g: &mut Gfx, ox: f32, al: f32) {
        if !self.out_list_shown() {
            return;
        }
        let (x, y, w, h) = (PANEL.0 + ox, PANEL.1, PANEL.2, PANEL.3);
        g.fill(&rrect(x, y, w, h, 14.0), &paint(22, 24, 32, al));
        let (main, bright) = (g.acc.main, g.acc.bright);
        for vi in 0..MAX_ROWS {
            let Some(d) = self.outs.list.get(self.out_scroll + vi) else { break };
            let (rx, ry) = row_pos(vi);
            let (rx, cy) = (rx + ox, ry + ROW_H / 2.0);
            let cur = Some(&d.id) == self.outs.cur.as_ref();
            if cur {
                g.fill(&rrect(rx + 4.0, ry + 1.0, w - 8.0, ROW_H - 2.0, 9.0), &paint(main[0], main[1], main[2], 0.24 * al));
                let p = paint(bright[0], bright[1], bright[2], al);
                let cx = rx + w - 20.0;
                g.line(cx - 5.0, cy, cx - 1.5, cy + 3.5, 2.0, &p);
                g.line(cx - 1.5, cy + 3.5, cx + 5.0, cy - 4.0, 2.0, &p);
            }
            draw_device(g, d, rx + 10.0, cy, w - 10.0 - 34.0, al);
        }
        let n = self.outs.list.len();
        if n > MAX_ROWS {
            // thin scrollbar at the right edge
            let (tx, ty, th) = (x + w - 5.0, y + 8.0, h - 16.0);
            let kh = th * MAX_ROWS as f32 / n as f32;
            let ky = ty + (th - kh) * self.out_scroll as f32 / (n - MAX_ROWS) as f32;
            g.fill(&rrect(tx, ky, 3.0, kh, 1.5), &paint(255, 255, 255, 0.3 * al));
        }
    }
}

// ---------------------------------------------------------------- Windows

/// `IPolicyConfig` (Windows 7 and later), undocumented: only `SetDefaultEndpoint` is called, the other methods are
/// declared to keep the vtable layout. Same declaration as EarTrumpet and SoundSwitch.
#[allow(dead_code, non_snake_case)]
mod policy {
    use std::ffi::c_void;
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    /// CLSID of CPolicyConfigClient.
    pub(super) const CLSID: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub(super) unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, period: *mut i64, min_period: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, id: PCWSTR, key: *const PROPERTYKEY, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, id: PCWSTR, key: *const PROPERTYKEY, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
    }

    /// The generated methods are private to this module.
    pub(super) unsafe fn set_default(pc: &IPolicyConfig, id: PCWSTR, role: ERole) -> HRESULT {
        pc.SetDefaultEndpoint(id, role)
    }

    /// Reads the device's mix format through the interface: true if `id` is a device it knows.
    #[cfg(test)]
    pub(super) unsafe fn knows(pc: &IPolicyConfig, id: PCWSTR) -> bool {
        let mut f: *mut c_void = std::ptr::null_mut();
        let ok = pc.GetMixFormat(id, &mut f).is_ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(f as *const c_void));
        ok
    }
}

/// PKEY_Device_FriendlyName (not in the enabled `windows` features).
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

fn policy_config() -> Option<policy::IPolicyConfig> {
    unsafe { CoCreateInstance(&policy::CLSID, None, CLSCTX_ALL).ok() }
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

/// A string COM allocated for us, then freed.
fn take(p: PWSTR) -> Option<String> {
    let s = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

/// Windows' device list and IPolicyConfig.
pub(crate) struct Sys;

impl Endpoints for Sys {
    fn devices(&self) -> Vec<Device> {
        let mut out = Vec::new();
        let Some(en) = enumerator() else { return out };
        unsafe {
            let Ok(col) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
            for i in 0..col.GetCount().unwrap_or(0) {
                let Ok(dev) = col.Item(i) else { continue };
                let Some(id) = dev.GetId().ok().and_then(take) else { continue };
                let props = dev.OpenPropertyStore(STGM_READ).ok();
                let name = props
                    .as_ref()
                    .and_then(|p| p.GetValue(&FRIENDLY_NAME).ok())
                    .map(|v| v.to_string())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| tr("Unknown device").to_string());
                // EndpointFormFactor: 3 = Headphones, 5 = Headset, 6 = Handset
                let form = props.as_ref().and_then(|p| p.GetValue(&PKEY_AudioEndpoint_FormFactor).ok()).and_then(|v| u32::try_from(&v).ok());
                out.push(Device { id, name, phones: matches!(form, Some(3 | 5 | 6)) });
            }
        }
        out
    }

    fn current(&self) -> Option<String> {
        unsafe { enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole).ok()?.GetId().ok().and_then(take) }
    }

    fn set_role(&self, id: &str, role: Role) -> bool {
        let role = match role {
            Role::Console => eConsole,
            Role::Multimedia => eMultimedia,
            Role::Communications => eCommunications,
        };
        let id = HSTRING::from(id);
        unsafe {
            let Some(pc) = policy_config() else { return false };
            policy::set_default(&pc, WPCWSTR(id.as_ptr()), role).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outs(ids: &[&str], cur: Option<&str>) -> Outputs {
        Outputs {
            list: ids.iter().map(|&id| Device { id: id.into(), name: id.into(), phones: false }).collect(),
            cur: cur.map(Into::into),
        }
    }

    #[test]
    fn names_split_into_device_and_driver() {
        assert_eq!(split_name("Speakers (Realtek(R) Audio)"), ("Speakers", "Realtek(R) Audio"));
        assert_eq!(split_name("Headphones (WH-1000XM4 Stereo)"), ("Headphones", "WH-1000XM4 Stereo"));
        assert_eq!(split_name("LG ULTRAGEAR (NVIDIA High Definition Audio)"), ("LG ULTRAGEAR", "NVIDIA High Definition Audio"));
        assert_eq!(split_name("Speakers"), ("Speakers", ""));
        assert_eq!(split_name("(Realtek)"), ("(Realtek)", ""), "nothing before the group: keep it whole");
        assert_eq!(split_name("Odd (name"), ("Odd (name", ""));
        assert_eq!(split_name(""), ("", ""));
    }

    #[test]
    fn next_wraps_around_both_ways() {
        let o = outs(&["a", "b", "c"], Some("b"));
        assert_eq!(o.next(1).map(|d| d.id.as_str()), Some("c"));
        assert_eq!(o.next(-1).map(|d| d.id.as_str()), Some("a"));
        let o = outs(&["a", "b", "c"], Some("c"));
        assert_eq!(o.next(1).map(|d| d.id.as_str()), Some("a"), "past the last: back to the first");
        let o = outs(&["a", "b", "c"], Some("a"));
        assert_eq!(o.next(-1).map(|d| d.id.as_str()), Some("c"));
    }

    #[test]
    fn next_needs_two_devices() {
        assert_eq!(outs(&["a"], Some("a")).next(1), None);
        assert_eq!(outs(&[], None).next(1), None);
        let o = outs(&["a", "b"], Some("gone"));
        assert_eq!(o.next(1).map(|d| d.id.as_str()), Some("a"), "unknown default: start at the first device");
    }

    #[test]
    fn read_and_current() {
        set_fake(&[("a", "Speakers (Realtek)", false), ("b", "Headphones (USB)", true)], Some("b"));
        let o = Outputs::read(&Fake::default());
        assert_eq!(o.list.len(), 2);
        assert_eq!(o.current().map(|d| d.name.as_str()), Some("Headphones (USB)"));
        assert!(o.current().unwrap().phones);
        set_fake(&[], None);
        assert_eq!(Outputs::read(&Fake::default()), Outputs::default());
    }

    #[test]
    fn switch_sets_all_three_roles() {
        set_fake(&[("a", "A", false), ("b", "B", false)], Some("a"));
        assert!(switch(&Fake::default(), "b"));
        let calls = with_fake(|f| f.calls.clone());
        assert_eq!(calls, ROLES.iter().map(|&r| ("b".to_string(), r)).collect::<Vec<_>>(), "Console, Multimedia and Communications");
        assert_eq!(Fake::default().current().as_deref(), Some("b"));

        set_fake(&[("a", "A", false), ("b", "B", false)], Some("a"));
        with_fake(|f| f.refuse = true);
        assert!(!switch(&Fake::default(), "b"), "a refused role is a failure");
    }

    /// On a real Windows desktop: the device list matches the default device, and setting the current default
    /// again through IPolicyConfig succeeds (this checks the undocumented interface layout without changing
    /// anything). With two or more devices it also switches to another one and back. CI only, so a local run
    /// never touches the developer's sound settings; a machine without playback devices is skipped.
    #[test]
    fn real_devices_switch_and_back() {
        if std::env::var_os("CI").is_none() {
            eprintln!("skipped: changes the default playback device, runs on CI only");
            return;
        }
        unsafe {
            // the app does this on its main thread; libtest threads start without COM
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let ep = Sys;
        // works without any audio device too: the object exists and an unknown id is refused without a crash
        assert!(policy_config().is_some(), "CPolicyConfigClient can be created");
        assert!(!ep.set_role("{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}", Role::Console), "unknown id is refused");
        let list = ep.devices();
        eprintln!("playback devices: {:?}", list.iter().map(|d| (&d.name, d.phones)).collect::<Vec<_>>());
        let Some(cur) = ep.current() else {
            assert!(list.is_empty(), "devices but no default");
            eprintln!("skipped: no playback device on this machine");
            return;
        };
        assert!(list.iter().any(|d| d.id == cur), "the default is one of the active devices");
        let pc = policy_config().unwrap();
        let wid = HSTRING::from(cur.as_str());
        assert!(unsafe { policy::knows(&pc, WPCWSTR(wid.as_ptr())) }, "the interface reads the real device (layout check)");
        let remote = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_REMOTESESSION) } != 0;
        if remote && !switch(&ep, &cur) {
            // Windows Sandbox / Remote Desktop: the redirected "Remote Audio" device cannot be made the default
            eprintln!("skipped switching: remote session, Windows refuses to set its remote audio device");
            return;
        }
        assert!(switch(&ep, &cur), "IPolicyConfig accepts the current default");
        assert_eq!(ep.current().as_deref(), Some(cur.as_str()));
        if let Some(other) = list.iter().find(|d| d.id != cur) {
            assert!(switch(&ep, &other.id));
            assert_eq!(ep.current().as_deref(), Some(other.id.as_str()), "switched");
            assert!(switch(&ep, &cur));
            assert_eq!(ep.current().as_deref(), Some(cur.as_str()), "and back");
        }
    }
}

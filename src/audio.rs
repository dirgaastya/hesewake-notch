//! Volume master perangkat audio default (IAudioEndpointVolume). Dipakai untuk kontrol volume lewat scroll di slide Music
//! dan (nanti) HUD volume. Dipanggil di UI thread; objek dibuat per panggilan (murah, dan mengikuti perangkat default).

use super::*;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use windows::core::{implement, PCWSTR as WPCWSTR};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

/// Pesan ke window: volume master berubah (`wParam` = persen | bisu << 8).
pub(crate) const WM_VOLUME: u32 = WM_APP + 6;

static TARGET: AtomicIsize = AtomicIsize::new(0);
/// Nilai terakhir (persen | bisu << 8): menghindari event ganda dan event dari perubahan kita sendiri.
static LAST: AtomicU32 = AtomicU32::new(u32::MAX);
static STARTED: AtomicBool = AtomicBool::new(false);

fn pack(pct: u8, muted: bool) -> u32 {
    pct as u32 | ((muted as u32) << 8)
}

pub(crate) fn unpack(v: usize) -> (u8, bool) {
    ((v & 0xff) as u8, (v >> 8) & 1 == 1)
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolCb;

impl IAudioEndpointVolumeCallback_Impl for VolCb_Impl {
    fn OnNotify(&self, pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        if pnotify.is_null() {
            return Ok(());
        }
        let d = unsafe { &*pnotify };
        let v = pack((d.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as u8, d.bMuted.as_bool());
        if LAST.swap(v, Ordering::Relaxed) != v {
            unsafe {
                let _ = PostMessageW(HWND(TARGET.load(Ordering::Relaxed) as *mut c_void), WM_VOLUME, WPARAM(v as usize), LPARAM(0));
            }
        }
        Ok(())
    }
}

/// Memberi tahu thread pemantau bahwa perangkat audio default berganti (pasang ulang callback).
#[implement(IMMNotificationClient)]
struct DevCb {
    tx: Sender<()>,
}

/// Tell the window that the playback devices changed, so the output list (output.rs) is read again.
fn post_output() {
    unsafe {
        let _ = PostMessageW(HWND(TARGET.load(Ordering::Relaxed) as *mut c_void), output::WM_OUTPUT, WPARAM(0), LPARAM(0));
    }
}

impl IMMNotificationClient_Impl for DevCb_Impl {
    fn OnDeviceStateChanged(&self, _id: &WPCWSTR, _state: DEVICE_STATE) -> windows::core::Result<()> {
        post_output();
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &WPCWSTR) -> windows::core::Result<()> {
        post_output();
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &WPCWSTR) -> windows::core::Result<()> {
        post_output();
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &WPCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            let _ = self.tx.send(());
            post_output();
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &WPCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Mulai memantau perubahan volume master (termasuk dari tombol keyboard / mixer). Thread latar belakang, tanpa polling.
pub(crate) fn start_watch(hwnd: isize) {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    TARGET.store(hwnd, Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let (tx, rx) = channel();
        let client: IMMNotificationClient = DevCb { tx }.into();
        let _ = en.RegisterEndpointNotificationCallback(&client);
        let cb: IAudioEndpointVolumeCallback = VolCb.into();
        let mut cur: Option<IAudioEndpointVolume> = None;
        loop {
            if let Some(old) = cur.take() {
                let _ = old.UnregisterControlChangeNotify(&cb);
            }
            if let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) {
                if let Ok(ep) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    if let (Ok(v), Ok(m)) = (ep.GetMasterVolumeLevelScalar(), ep.GetMute()) {
                        LAST.store(pack((v * 100.0).round() as u8, m.as_bool()), Ordering::Relaxed);
                    }
                    let _ = ep.RegisterControlChangeNotify(&cb);
                    cur = Some(ep);
                }
            }
            if rx.recv().is_err() {
                break; // tidak mungkin terjadi selama `client` hidup
            }
        }
    });
}

fn endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// (volume 0..1, bisu).
#[allow(dead_code)]
pub(crate) fn get() -> Option<(f32, bool)> {
    let ep = endpoint()?;
    unsafe { Some((ep.GetMasterVolumeLevelScalar().ok()?, ep.GetMute().ok()?.as_bool())) }
}

/// Ubah volume sebesar `delta` (-1..1) lalu kembalikan keadaan baru. Menyalakan kembali suara bila sedang bisu dan
/// volume dinaikkan.
pub(crate) fn adjust(delta: f32) -> Option<(f32, bool)> {
    let ep = endpoint()?;
    unsafe {
        let cur = ep.GetMasterVolumeLevelScalar().ok()?;
        let new = (cur + delta).clamp(0.0, 1.0);
        let was_muted = ep.GetMute().ok()?.as_bool();
        let unmute = delta > 0.0 && was_muted;
        // perubahan dari kita sendiri tidak perlu dilaporkan ulang oleh callback; catat hasilnya SEBELUM mengubah,
        // karena callback bisa tiba selagi kita masih di dalam Set*
        LAST.store(pack((new * 100.0).round() as u8, was_muted && !unmute), Ordering::Relaxed);
        ep.SetMasterVolumeLevelScalar(new, std::ptr::null()).ok()?;
        if unmute {
            ep.SetMute(false, std::ptr::null()).ok()?;
        }
        let muted = ep.GetMute().ok()?.as_bool();
        LAST.store(pack((new * 100.0).round() as u8, muted), Ordering::Relaxed);
        Some((new, muted))
    }
}

/// Langkah volume (0..1) untuk satu muatan roda mouse (`delta` Windows: 120 = satu takik).
pub(crate) fn wheel_step(delta: i32) -> f32 {
    (delta as f32 / 120.0) * 0.02
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_step_is_two_percent_per_notch() {
        assert!((wheel_step(120) - 0.02).abs() < 1e-6);
        assert!((wheel_step(-240) + 0.04).abs() < 1e-6);
        assert!((wheel_step(30) - 0.005).abs() < 1e-6);
    }
}

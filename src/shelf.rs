//! Fitur Ledge: rak file (maks 5 item), drag & drop, slide Ledge.

use super::*;

use windows::core::Interface;
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
use tiny_skia::{PixmapPaint, StrokeDash};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

const SHELF_NAME: &str = "Shelf";

const SHELF_MAX: usize = 5; // folder / zip dihitung 1 item
pub(crate) const SH_GAP: f32 = 8.0;
pub(crate) const SH_W: f32 = (CW - SH_GAP * 4.0) / SHELF_MAX as f32;
pub(crate) const SH_Y: f32 = 38.0;
pub(crate) const SH_H: f32 = 100.0;

const ICON_PX: i32 = 48;

/// Garis putus-putus (panjang `dash` px, bergeser `off`) di sekeliling `path`.
pub(crate) fn dashed(g: &mut Gfx, path: &Path, p: &Paint, w: f32, dash: f32, off: f32) {
    if let Some(d) = StrokeDash::new(vec![dash, dash], off) {
        let st = Stroke { width: w, dash: Some(d), ..Default::default() };
        g.pm.stroke_path(path, p, &st, g.ts, None);
    }
}

// ---------------------------------------------------------------- shelf data
pub(crate) struct ShelfItem {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) icon: Option<Pixmap>,
    /// Kapan item ini masuk (untuk animasi pop saat baru di-drop).
    pub(crate) born: Instant,
    /// Kapan item dibuka (membal sebentar).
    pub(crate) bump: Option<Instant>,
}

impl ShelfItem {
    pub(crate) fn new(path: &str) -> Self {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        ShelfItem { path: path.to_string(), name, icon: load_file_icon(path), born: Instant::now(), bump: None }
    }
}

/// Ikon berwarna dari Windows Shell (sama dengan yang terlihat di Explorer).
/// Dimuat sekali saat item dibuat, bukan setiap frame.
fn load_file_icon(path: &str) -> Option<Pixmap> {
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None::<&IBindCtx>).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let hbmp = factory.GetImage(SIZE { cx: ICON_PX, cy: ICON_PX }, SIIGBF_ICONONLY).ok()?;

        let mut bm = BITMAP::default();
        GetObjectW(
            HGDIOBJ(hbmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            let _ = DeleteObject(hbmp);
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let n = GetDIBits(dc, hbmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(hbmp);
        if n == 0 {
            return None;
        }

        // Jika tidak ada alpha sama sekali, anggap opaque.
        let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
        let mut pm = Pixmap::new(w as u32, h as u32)?;
        for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(buf.chunks_exact(4)) {
            d[0] = s[2]; // BGRA -> RGBA
            d[1] = s[1];
            d[2] = s[0];
            d[3] = if has_alpha { s[3] } else { 255 };
        }
        Some(pm)
    }
}

pub(crate) fn load_shelf() -> Vec<ShelfItem> {
    std::fs::read_to_string(data_path("shelf.txt"))
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .take(SHELF_MAX)
                .map(|p| {
                    // item yang dimuat saat start tidak perlu animasi pop
                    let mut it = ShelfItem::new(p);
                    it.born = Instant::now() - Duration::from_secs(5);
                    it
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn save_shelf(v: &[ShelfItem]) {
    write_data("shelf.txt", v.iter().map(|i| format!("{}\n", i.path)).collect());
}

pub(crate) fn open_path(path: &str) {
    unsafe {
        let _ = ShellExecuteW(
            HWND::default(),
            w!("open"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

impl App {
    /// Tambah path hasil drop ke shelf (duplikat diabaikan). Set flash "full" bila limit tercapai.
    pub(crate) fn shelf_add(&mut self, paths: Vec<String>) {
        let mut changed = false;
        for p in paths {
            if self.shelf.iter().any(|s| s.path.eq_ignore_ascii_case(&p)) {
                continue;
            }
            if self.shelf.len() >= SHELF_MAX {
                self.shelf_flash = Some(Instant::now());
                break;
            }
            self.shelf.push(ShelfItem::new(&p));
            changed = true;
        }
        if !changed {
            return;
        }
        save_shelf(&self.shelf);
        // Picu animasi droplet: tepi bawah notch melorot lalu memantul kembali.
        self.drop_pulse = Some(Instant::now());
        self.last = Instant::now();
        unsafe {
            schedule(self);
            present(self);
        }
    }

    /// Detik sejak item di-drop selama animasi gelombang berjalan; None bila tidak ada animasi.
    pub(crate) fn drop_t(&self) -> Option<f32> {
        let e = self.drop_pulse?.elapsed().as_secs_f32();
        if e < DROP_ANIM_SECS { Some(e) } else { None }
    }

    /// Sebuah file sedang diseret dari luar ke atas notch: tombol kiri mouse ditahan, kursor di zona rak, dan tekanan itu
    /// tidak dimulai di notch sendiri. Dipakai untuk sorotan "Drop to add".
    pub(crate) fn shelf_drag_hover(&self) -> bool {
        if !(self.expanded() && self.is_slide(SlideId::Ledge)) || self.drag.is_some() || self.shelf_drag.is_some() {
            return false;
        }
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0;
        down && self.cursor_canvas().is_some_and(|(x, y)| in_rect(x, y, LX - 10.0, SH_Y - 10.0, CW + 20.0, SH_H + 20.0))
    }

    /// Item rak yang bisa diklik atau diseret keluar (kursor tangan).
    pub(crate) fn shelf_hand(&self, x: f32, y: f32) -> bool {
        (0..self.shelf.len()).any(|i| in_rect(x, y, LX + i as f32 * (SH_W + SH_GAP), SH_Y, SH_W, SH_H))
    }

    pub(crate) fn shelf_animating(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Ledge) && (self.shelf_drag_hover() || self.shelf.iter().any(|i| i.born.elapsed().as_secs_f32() < 0.85 || i.bump.is_some_and(|b| b.elapsed().as_secs_f32() < 0.5)))
    }

    pub(crate) fn slide_shelf(&self, g: &mut Gfx, ox: f32, al: f32) {
        let x = LX + ox;
        let t = self.t;
        let cur = self.cursor_canvas();
        let drag = self.shelf_drag_hover();
        let acc = g.acc;
        g.txtb(SHELF_NAME, x, 12.0, 15.0, [255, 255, 255], al, 100.0);

        let full = self.shelf_flash.map_or(false, |t| t.elapsed() < Duration::from_millis(1800));
        let label = if drag {
            tr("Release to put it on the shelf").to_string()
        } else if full {
            fill(tr("{} is full"), &[&SHELF_NAME])
        } else {
            format!("{}/{}", self.shelf.len(), SHELF_MAX)
        };
        let lw = g.width_ex(&label, 13.0, true);
        g.txtb(&label, RX + ox - lw, 14.0, 13.0, if drag { acc.bright } else { acc.main }, al, 160.0);

        // Zona drop: hampir tak terlihat; saat file diseret ke atasnya menyala dengan garis putus-putus yang berjalan.
        if drag {
            let glow = 0.09 + 0.04 * (t * 6.0).sin();
            g.fill(&rrect(x - 6.0, SH_Y - 6.0, CW + 12.0, SH_H + 12.0, 16.0), &paint(acc.main[0], acc.main[1], acc.main[2], glow * al));
            dashed(g, &rrect(x - 6.0, SH_Y - 6.0, CW + 12.0, SH_H + 12.0, 16.0), &paint(acc.bright[0], acc.bright[1], acc.bright[2], 0.9 * al), 1.5, 7.0, -t * 28.0);
        } else {
            g.stroke(&rrect(x, SH_Y, CW, SH_H, 14.0), &paint(255, 255, 255, 0.06 * al), 1.0);
        }

        if self.draw_ledge_downloads(g, ox, al) {
            return; // the shelf items come back when the downloads are done
        }

        // Slot kosong: garis putus-putus dan tanda "+"; saat drag, slot berikutnya menyala dan "+" berdenyut.
        let n = self.shelf.len();
        let empty_slot = |g: &mut Gfx, i: usize| {
            let tx = x + i as f32 * (SH_W + SH_GAP);
            let next = drag && i == n;
            let (cx, cy) = (tx + SH_W / 2.0, SH_Y + SH_H / 2.0 - if n == 0 { 12.0 } else { 0.0 });
            if next {
                g.fill(&rrect(tx, SH_Y + 4.0, SH_W, SH_H - 8.0, 12.0), &paint(acc.main[0], acc.main[1], acc.main[2], 0.2 * al));
            }
            let oc = if next { acc.bright } else { [255, 255, 255] };
            dashed(g, &rrect(tx + 0.5, SH_Y + 4.5, SH_W - 1.0, SH_H - 9.0, 11.5), &paint(oc[0], oc[1], oc[2], if next { 0.9 } else { 0.13 } * al), 1.2, 4.0, 0.0);
            let s = if next { 1.0 + 0.12 * (t * 8.0).sin() } else { 1.0 };
            let p = paint(oc[0], oc[1], oc[2], if next { 1.0 } else { 0.28 } * al);
            g.line(cx - 6.0 * s, cy, cx + 6.0 * s, cy, 1.6, &p);
            g.line(cx, cy - 6.0 * s, cx, cy + 6.0 * s, 1.6, &p);
        };
        if self.shelf.is_empty() {
            // kosong: satu zona putus-putus dengan "+" dan petunjuk di tengah (bukan lima slot yang tertimpa teks)
            let oc = if drag { acc.bright } else { [255, 255, 255] };
            dashed(g, &rrect(x + 0.5, SH_Y + 4.5, CW - 1.0, SH_H - 9.0, 14.0), &paint(oc[0], oc[1], oc[2], if drag { 0.9 } else { 0.16 } * al), 1.2, 5.0, 0.0);
            let (cx, cy) = (CX + ox, SH_Y + SH_H / 2.0 - 12.0);
            let s = if drag { 1.0 + 0.12 * (t * 8.0).sin() } else { 1.0 };
            g.circle(cx, cy, 13.0, &paint(oc[0], oc[1], oc[2], 0.08 * al));
            let p = paint(oc[0], oc[1], oc[2], if drag { 1.0 } else { 0.4 } * al);
            g.line(cx - 6.0 * s, cy, cx + 6.0 * s, cy, 1.6, &p);
            g.line(cx, cy - 6.0 * s, cx, cy + 6.0 * s, 1.6, &p);
            g.txt_c(tr("Drop files or folders here"), cx, cy + 22.0, 13.0, [160, 172, 200], al, 300.0, false);
            return;
        }

        for i in 0..self.shelf.len() {
            let tx = x + i as f32 * (SH_W + SH_GAP);
            let it = &self.shelf[i];
            let cx = tx + SH_W / 2.0;

            // masuk dengan pop (memantul dari bawah) saat baru di-drop; hover mengangkat tampilan dan memberi garis aksen
            let age = it.born.elapsed().as_secs_f32();
            let pop = if age < 0.8 { (1.0 - (-9.0 * age).exp() * (18.0 * age).cos()).clamp(0.0, 1.2) } else { 1.0 };
            let ial = al * (age / 0.2).clamp(0.0, 1.0);
            let bmp = it.bump.map_or(0.0, |b| { let e = b.elapsed().as_secs_f32(); if e < 0.5 { -6.0 * (-8.0 * e).exp() * (e * 20.0).sin().abs() } else { 0.0 } });
            let dy = (1.0 - pop) * 14.0 + bmp;
            let hov = cur.is_some_and(|(hx, hy)| in_rect(hx, hy, tx, SH_Y, SH_W, SH_H));
            g.fill(&rrect(tx, SH_Y + 4.0 + dy, SH_W, SH_H - 8.0, 12.0), &paint(255, 255, 255, (0.08 + if hov { 0.06 } else { 0.0 }) * ial));
            if hov {
                g.stroke(&rrect(tx + 0.5, SH_Y + 4.5 + dy, SH_W - 1.0, SH_H - 9.0, 11.5), &paint(acc.main[0], acc.main[1], acc.main[2], 0.6 * ial), 1.2);
            }

            // Ikon berwarna dari shell Windows.
            if let Some(ic) = &it.icon {
                let ix = (cx - ic.width() as f32 / 2.0).round() as i32;
                let iy = (SH_Y + 10.0 + dy) as i32;
                g.pm.draw_pixmap(
                    ix,
                    iy,
                    ic.as_ref(),
                    &PixmapPaint { opacity: ial, ..Default::default() },
                    g.ts,
                    None,
                );
            }

            // Nama file di bawah ikon (maks 2 baris).
            let lines = wrap_lines(g, &it.name, 11.0, SH_W - 12.0);
            let y0 = SH_Y + 64.0 + dy;
            for (li, l) in lines.iter().take(2).enumerate() {
                let s = if li == 1 && lines.len() > 2 { ellipsize(g, l, 11.0, SH_W - 12.0) } else { l.clone() };
                g.txt_c(&s, cx, y0 + li as f32 * 14.0, 11.0, [235, 235, 238], ial, SH_W - 8.0, false);
            }

            // Tombol hapus kecil (posisinya sama dengan hit-area di input.rs); membulat saat kartu dihover.
            let (bx, by) = (tx + SH_W - 10.0, SH_Y + 10.0 + dy);
            if hov {
                g.circle(bx, by, 8.0, &paint(255, 255, 255, 0.22 * ial));
            }
            let wh = paint(255, 255, 255, (if hov { 0.95 } else { 0.5 }) * ial);
            g.line(bx - 2.4, by - 2.4, bx + 2.4, by + 2.4, 1.3, &wh);
            g.line(bx - 2.4, by + 2.4, bx + 2.4, by - 2.4, 1.3, &wh);
        }
        for i in self.shelf.len()..SHELF_MAX {
            empty_slot(g, i);
        }
    }
}
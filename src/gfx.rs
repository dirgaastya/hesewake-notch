//! Rendering dasar: font/teks, kanvas Gfx, bentuk & helper gambar.

use super::*;

/// Decode gambar dengan batas alokasi (32 MB): cover/ikon berukuran piksel sangat besar ditolak, bukan dibongkar
/// ke RGBA penuh yang bisa memakan puluhan MB.
pub(crate) fn decode_limited(bytes: &[u8]) -> Option<image::DynamicImage> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut lim = image::Limits::default();
    lim.max_alloc = Some(32 * 1024 * 1024);
    r.limits(lim);
    r.decode().ok()
}

pub(crate) fn pixmap_from_rgba(rgba: &[u8], size: u32) -> Option<Pixmap> {
    let mut pm = Pixmap::new(size, size)?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        let al = s[3] as u32;
        d[0] = (s[0] as u32 * al / 255) as u8;
        d[1] = (s[1] as u32 * al / 255) as u8;
        d[2] = (s[2] as u32 * al / 255) as u8;
        d[3] = s[3];
    }
    Some(pm)
}

// ---------------------------------------------------------------- text
static FONT_REG: &[u8] = include_bytes!("../assets/InstrumentSans-Regular.ttf");
static FONT_REG_EXT: &[u8] = include_bytes!("../assets/InstrumentSans-Ext-Regular.ttf");
static FONT_BOLD: &[u8] = include_bytes!("../assets/InstrumentSans-Bold.ttf");
static FONT_BOLD_EXT: &[u8] = include_bytes!("../assets/InstrumentSans-Ext-Bold.ttf");

const ASC: f32 = 1.079;
// Cukup besar supaya semua slide (beberapa ukuran, reguler+bold) muat tanpa clear() yang memicu rasterisasi ulang.
const GLYPH_CACHE_MAX: usize = 2000;

pub(crate) type GlyphKey = (char, u32, bool);

pub(crate) struct Text {
    pub(crate) reg: Vec<fontdue::Font>,
    pub(crate) bold: Vec<fontdue::Font>,
    pub(crate) cache: HashMap<GlyphKey, (fontdue::Metrics, Vec<u8>)>,
    /// Skala DPI: glyph dirasterisasi di `size * s`, ukuran yang dikembalikan tetap piksel logis.
    pub(crate) s: f32,
}

fn load_font(data: &[u8]) -> Option<fontdue::Font> {
    fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).ok()
}

fn glyph_of<'a>(
    cache: &'a mut HashMap<GlyphKey, (fontdue::Metrics, Vec<u8>)>,
    chain: &[fontdue::Font],
    ch: char,
    size: f32,
    bold: bool,
) -> &'a (fontdue::Metrics, Vec<u8>) {
    let key = (ch, size as u32, bold);
    if !cache.contains_key(&key) && cache.len() >= GLYPH_CACHE_MAX {
        cache.clear();
    }
    cache.entry(key).or_insert_with(|| {
        match chain.iter().find(|f| f.lookup_glyph_index(ch) != 0) {
            Some(f) => f.rasterize(ch, size),
            // kana / kanji / hanzi / hangul: font CJK Windows lewat GDI (lihat cjk.rs)
            None => cjk::rasterize(ch, size, bold).unwrap_or_else(|| chain[0].rasterize(ch, size)),
        }
    })
}

impl Text {
    pub(crate) fn load() -> Option<Text> {
        let mut reg = vec![load_font(FONT_REG)?];
        let mut bold = vec![load_font(FONT_BOLD)?];
        reg.extend(load_font(FONT_REG_EXT));
        bold.extend(load_font(FONT_BOLD_EXT));
        Some(Text { reg, bold, cache: HashMap::new(), s: 1.0 })
    }

    /// Rasterisasi awal karakter umum pada ukuran yang dipakai UI, supaya frame pertama saat expand tidak melonjak.
    pub(crate) fn warm(&mut self) {
        const TEXT: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,:-'()/%+";
        for &(size, bold) in &[(11.0, false), (12.0, false), (13.0, false), (13.0, true), (14.0, true), (15.0, true), (16.0, true), (17.0, true)] {
            self.measure(TEXT, size, bold);
        }
        self.measure("0123456789:", 46.0, true);
        self.measure("0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ", 12.0, true);
    }

    /// `s` dipotong lalu diberi "…" sehingga lebarnya <= `maxw`. Satu putaran (jumlah advance per karakter), hasilnya
    /// sama dengan memangkas karakter satu per satu sambil mengukur ulang seluruh string.
    pub(crate) fn ellipsize(&mut self, s: &str, size: f32, bold: bool, maxw: f32) -> String {
        let (size, maxw) = (size * self.s, maxw * self.s);
        let chain: &[fontdue::Font] = if bold { &self.bold } else { &self.reg };
        let ell = glyph_of(&mut self.cache, chain, '\u{2026}', size, bold).0.advance_width;
        let (mut acc, mut keep) = (0.0f32, 0usize);
        for (i, ch) in s.char_indices() {
            let adv = glyph_of(&mut self.cache, chain, ch, size, bold).0.advance_width;
            if acc + adv + ell > maxw {
                break;
            }
            acc += adv;
            keep = i + ch.len_utf8();
        }
        let mut out = s[..keep].to_string();
        out.push('\u{2026}');
        out
    }

    fn measure(&mut self, s: &str, size: f32, bold: bool) -> f32 {
        let size = size * self.s;
        let chain: &[fontdue::Font] = if bold { &self.bold } else { &self.reg };
        let mut total = 0.0;
        for ch in s.chars() {
            let (m, _) = glyph_of(&mut self.cache, chain, ch, size, bold);
            total += m.advance_width;
        }
        total / self.s
    }

    pub(crate) fn draw(
        &mut self,
        pm: &mut Pixmap,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        col: [u8; 3],
        alpha: f32,
        max_w: f32,
        bold: bool,
        clip: Option<(f32, f32, f32)>,
    ) {
        if alpha <= 0.02 {
            return;
        }
        // Semua masukan logis -> piksel fisik (s = 1 tidak mengubah apa pun).
        let k = self.s;
        let (x, y, size, max_w) = (x * k, y * k, size * k, max_w * k);
        let clip = clip.map(|(cl, cr, fd)| (cl * k, cr * k, fd * k));
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        let chain: &[fontdue::Font] = if bold { &self.bold } else { &self.reg };
        let asc = size * ASC;
        let mut pen = 0.0f32;
        let data = pm.data_mut();
        let lut = if is_light(col) { Some(boost_lut()) } else { None };
        for ch in s.chars() {
            let (m, bm) = glyph_of(&mut self.cache, chain, ch, size, bold);
            if pen + m.advance_width > max_w {
                break;
            }
            let gx = (x + pen).round() as i32 + m.xmin;
            let gy = (y + asc).round() as i32 - m.height as i32 - m.ymin;
            let skip = match clip {
                Some((cl, cr, _)) => ((gx + m.width as i32) as f32) < cl || (gx as f32) > cr,
                None => false,
            };
            if !skip {
                for row in 0..m.height {
                    for colx in 0..m.width {
                        let cov = bm[row * m.width + colx];
                        if cov == 0 {
                            continue;
                        }
                        let cov = lut.map_or(cov, |l| l[cov as usize]);
                        let (px, py) = (gx + colx as i32, gy + row as i32);
                        if px < 0 || py < 0 || px >= pw || py >= ph {
                            continue;
                        }
                        let mut a = cov as f32 / 255.0 * alpha;
                        if let Some((cl, cr, fd)) = clip {
                            let fx = px as f32 + 0.5;
                            if fx < cl || fx >= cr {
                                continue;
                            }
                            if fd > 0.0 {
                                a *= ((fx - cl) / fd).clamp(0.0, 1.0) * ((cr - fx) / fd).clamp(0.0, 1.0);
                            }
                        }
                        let i = ((py * pw + px) * 4) as usize;
                        for c in 0..3 {
                            data[i + c] = (col[c] as f32 * a + data[i + c] as f32 * (1.0 - a)) as u8;
                        }
                        data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)) as u8;
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}

/// Teks terang di atas latar gelap tampak lebih tipis daripada sebaliknya karena campuran alfa linear; tepi glyph
/// diperkuat dengan kurva ringan agar goresan 1 px tetap terbaca.
fn is_light(col: [u8; 3]) -> bool {
    (col[0] as u32 * 299 + col[1] as u32 * 587 + col[2] as u32 * 114) / 1000 > 128
}

fn boost_lut() -> &'static [u8; 256] {
    static LUT: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (255.0 * (i as f32 / 255.0).powf(0.7)).round().clamp(0.0, 255.0) as u8;
        }
        t
    })
}

// ---------------------------------------------------------------- drawing primitives
/// Pixel box (x0, y0, x1, y1), end-exclusive. Used so we only clear / mask / copy what changed.
pub(crate) type Box4 = (usize, usize, usize, usize);

pub(crate) fn union_box(a: Box4, b: Box4) -> Box4 {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

pub(crate) struct Gfx {
    pub(crate) pm: Pixmap,
    pub(crate) text: Option<Text>,
    pub(crate) mask: Option<Mask>,
    /// Region touched by the previous frame (so it gets cleared on the next one).
    pub(crate) prev_box: Box4,
    /// Region that must be copied to the window bitmap for the current frame.
    pub(crate) dirty: Box4,
    /// Skala DPI dan transform-nya: semua gambar memakai koordinat logis (lihat scale.rs).
    pub(crate) s: f32,
    pub(crate) ts: Transform,
    /// The user's accent color; refreshed from Settings at the start of every frame (render::draw).
    pub(crate) acc: Accent,
}

impl Gfx {
    /// Kanvas kosong untuk skala `s` (prev_box/dirty = seluruh kanvas, kotak dalam piksel fisik).
    pub(crate) fn new(s: f32) -> Gfx {
        let (w, h) = scale::canvas(s);
        let full = (0, 0, w as usize, h as usize);
        Gfx {
            pm: Pixmap::new(w as u32, h as u32).expect("kanvas"),
            text: Text::load().map(|mut t| {
                t.s = s;
                t
            }),
            mask: Mask::new(w as u32, h as u32),
            prev_box: full,
            dirty: full,
            s,
            ts: Transform::from_scale(s, s),
            acc: accent::get(0),
        }
    }

    /// Canvas of `w` x `h` logical px at scale `s` (the Settings window; the notch canvas comes from `new`).
    pub(crate) fn sized(s: f32, w: f32, h: f32) -> Gfx {
        let (pw, ph) = ((w * s).ceil().max(1.0) as u32, (h * s).ceil().max(1.0) as u32);
        let full = (0, 0, pw as usize, ph as usize);
        Gfx {
            pm: Pixmap::new(pw, ph).expect("kanvas"),
            text: Text::load().map(|mut t| {
                t.s = s;
                t
            }),
            mask: None,
            prev_box: full,
            dirty: full,
            s,
            ts: Transform::from_scale(s, s),
            acc: accent::get(0),
        }
    }

    /// Paint in the main accent color with alpha `a`.
    pub(crate) fn sacc(&self, a: f32) -> Paint<'static> {
        let c = self.acc.main;
        paint(c[0], c[1], c[2], a)
    }

    /// Paint in the bright accent color (thin lines and small text) with alpha `a`.
    pub(crate) fn bright(&self, a: f32) -> Paint<'static> {
        let c = self.acc.bright;
        paint(c[0], c[1], c[2], a)
    }

    pub(crate) fn clear_box(&mut self, b: Box4) {
        let pw = self.pm.width() as usize;
        let ph = self.pm.height() as usize;
        let (x0, y0, x1, y1) = (b.0, b.1, b.2.min(pw), b.3.min(ph));
        if x1 <= x0 {
            return;
        }
        let data = self.pm.data_mut();
        for y in y0..y1 {
            data[(y * pw + x0) * 4..(y * pw + x1) * 4].fill(0);
        }
    }

    pub(crate) fn fill(&mut self, path: &Path, p: &Paint) {
        self.pm.fill_path(path, p, FillRule::Winding, self.ts, None);
    }
    pub(crate) fn stroke(&mut self, path: &Path, p: &Paint, w: f32) {
        let st = Stroke { width: w, ..Default::default() };
        self.pm.stroke_path(path, p, &st, self.ts, None);
    }
    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, p: &Paint) {
        // Fill through a path, not `Pixmap::fill_rect`: tiny-skia 0.11's anti-aliased fill_rect (only used at 100%
        // scale) hits a debug_assert for a 1-2 px wide rect that covers no pixel fully, e.g. the "+" icon in
        // To-do. That panics in debug builds.
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.fill(&PathBuilder::from_rect(r), p);
        }
    }
    pub(crate) fn circle(&mut self, x: f32, y: f32, r: f32, p: &Paint) {
        if let Some(path) = PathBuilder::from_circle(x, y, r) {
            self.fill(&path, p);
        }
    }
    pub(crate) fn poly(&mut self, pts: &[(f32, f32)], p: &Paint) {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for q in &pts[1..] {
            pb.line_to(q.0, q.1);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            self.fill(&path, p);
        }
    }
    pub(crate) fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, wd: f32, p: &Paint) {
        let (dx, dy) = (x2 - x1, y2 - y1);
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / len * wd / 2.0, dx / len * wd / 2.0);
        self.poly(&[(x1 + nx, y1 + ny), (x2 + nx, y2 + ny), (x2 - nx, y2 - ny), (x1 - nx, y1 - ny)], p);
        self.circle(x1, y1, wd / 2.0, p);
        self.circle(x2, y2, wd / 2.0, p);
    }
    fn txt_ex(&mut self, s: &str, x: f32, y: f32, size: f32, col: [u8; 3], al: f32, max_w: f32, bold: bool) {
        if let Some(t) = self.text.as_mut() {
            t.draw(&mut self.pm, s, x, y, size, col, al, max_w, bold, None);
        }
    }
    pub(crate) fn txt_clip(&mut self, s: &str, x: f32, y: f32, size: f32, col: [u8; 3], al: f32, cl: f32, cr: f32, fade: f32) {
        if let Some(t) = self.text.as_mut() {
            t.draw(&mut self.pm, s, x, y, size, col, al, f32::INFINITY, false, Some((cl, cr, fade)));
        }
    }
    pub(crate) fn txt(&mut self, s: &str, x: f32, y: f32, size: f32, col: [u8; 3], al: f32, max_w: f32) {
        self.txt_ex(s, x, y, size, col, al, max_w, false);
    }
    pub(crate) fn txtb(&mut self, s: &str, x: f32, y: f32, size: f32, col: [u8; 3], al: f32, max_w: f32) {
        self.txt_ex(s, x, y, size, col, al, max_w, true);
    }
    pub(crate) fn width_ex(&mut self, s: &str, size: f32, bold: bool) -> f32 {
        self.text.as_mut().map(|t| t.measure(s, size, bold)).unwrap_or(0.0)
    }
    pub(crate) fn width(&mut self, s: &str, size: f32) -> f32 {
        self.width_ex(s, size, false)
    }
    pub(crate) fn txt_c(&mut self, s: &str, cx: f32, y: f32, size: f32, col: [u8; 3], al: f32, max_w: f32, bold: bool) {
        let tw = self.width_ex(s, size, bold).min(max_w);
        self.txt_ex(s, cx - tw / 2.0, y, size, col, al, max_w, bold);
    }
    pub(crate) fn ring(&mut self, cx: f32, cy: f32, r: f32, frac: f32, wd: f32, track: &Paint, fg: &Paint) {
        if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
            let st = Stroke { width: wd, ..Default::default() };
            self.pm.stroke_path(&path, track, &st, self.ts, None);
        }
        if frac > 0.001 {
            let steps = ((frac * 64.0).ceil() as i32).max(2);
            let mut pb = PathBuilder::new();
            for s in 0..=steps {
                let ang = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac.min(1.0) * s as f32 / steps as f32;
                let (px, py) = (cx + r * ang.cos(), cy + r * ang.sin());
                if s == 0 {
                    pb.move_to(px, py);
                } else {
                    pb.line_to(px, py);
                }
            }
            if let Some(path) = pb.finish() {
                let st = Stroke { width: wd, line_cap: LineCap::Round, ..Default::default() };
                self.pm.stroke_path(&path, fg, &st, self.ts, None);
            }
        }
    }
}

pub(crate) fn dummy_path() -> Path {
    PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap())
}

pub(crate) fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish().unwrap_or_else(dummy_path)
}

pub(crate) fn notch_path(w: f32, h: f32) -> Path {
    notch_path_w(w, h, 0.0)
}

/// Bentuk notch yang sama, tapi tepi bawah "melorot" sebesar `dip` px (pulse droplet).
/// `dip = 0` identik dengan bentuk normal; `dip < 0` berarti tepi memantul ke atas.
pub(crate) fn notch_path_w(w: f32, h: f32, dip: f32) -> Path {
    let dip = dip.min(H as f32 - 1.0 - h); // jangan keluar dari kanvas
    let ear = (h * 0.35).min(10.0);
    let rb = (h * 0.5).min(22.0);
    let l = CX - w / 2.0;
    let r = CX + w / 2.0;
    let mut pb = PathBuilder::new();
    pb.move_to(l - ear, 0.0);
    pb.line_to(r + ear, 0.0);
    pb.quad_to(r, 0.0, r, ear);
    pb.line_to(r, h - rb);
    pb.quad_to(r, h, r - rb, h);
    if dip.abs() < 0.2 {
        pb.line_to(l + rb, h);
    } else {
        // Tonjolan tetesan di tengah tepi bawah; makin turun makin ramping (leher), memantul melebar.
        let a = r - rb;
        let b = l + rb;
        let base = ((r - l) * 0.30).min(110.0).max(24.0);
        let bw = (base * (1.0 - 0.3 * (dip / DROP_AMP).clamp(-0.5, 1.0))).min((a - b) / 2.0 - 1.0).max(4.0);
        pb.line_to(CX + bw, h);
        pb.cubic_to(CX + bw * 0.55, h, CX + bw * 0.50, h + dip, CX, h + dip);
        pb.cubic_to(CX - bw * 0.50, h + dip, CX - bw * 0.55, h, CX - bw, h);
        pb.line_to(b, h);
    }
    pb.quad_to(l, h, l, h - rb);
    pb.line_to(l, ear);
    pb.quad_to(l, 0.0, l - ear, 0.0);
    pb.close();
    pb.finish().unwrap_or_else(dummy_path)
}

pub(crate) fn paint(r: u8, g: u8, b: u8, a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8);
    p.anti_alias = true;
    p
}

pub(crate) fn acc(a: f32) -> Paint<'static> {
    paint(ACCENT[0], ACCENT[1], ACCENT[2], a)
}

pub(crate) fn in_rect(x: f32, y: f32, rx: f32, ry: f32, rw: f32, rh: f32) -> bool {
    x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
}

pub(crate) fn ty(h: f32, size: f32) -> f32 {
    (h - size * 1.33) / 2.0
}

/// Greedy word wrap (breaks long words by character). '\n' always starts a new line.
pub(crate) fn wrap_lines(g: &mut Gfx, s: &str, size: f32, maxw: f32) -> Vec<String> {
    wrap_lines_ex(g, s, size, maxw, false)
}

/// Seperti `wrap_lines`, tapi dengan ukuran huruf tebal bila `bold`.
pub(crate) fn wrap_lines_ex(g: &mut Gfx, s: &str, size: f32, maxw: f32, bold: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        let mut lw = 0.0f32;
        let mut last_sp: Option<usize> = None;
        for ch in para.chars() {
            let mut b = [0u8; 4];
            let cw = g.width_ex(ch.encode_utf8(&mut b), size, bold);
            if lw + cw > maxw && !line.is_empty() {
                match last_sp {
                    Some(sp) if ch != ' ' => {
                        let rest = line[sp + 1..].to_string();
                        line.truncate(sp);
                        out.push(std::mem::replace(&mut line, rest));
                        lw = g.width_ex(&line, size, bold);
                    }
                    _ => {
                        out.push(std::mem::take(&mut line));
                        lw = 0.0;
                        if ch == ' ' {
                            last_sp = None;
                            continue;
                        }
                    }
                }
                last_sp = None;
            }
            if ch == ' ' {
                last_sp = Some(line.len());
            }
            line.push(ch);
            lw += cw;
        }
        out.push(line);
    }
    out
}

/// Cuts `s` so that `s + "..."` fits in `maxw`.
pub(crate) fn ellipsize(g: &mut Gfx, s: &str, size: f32, maxw: f32) -> String {
    match g.text.as_mut() {
        Some(t) => t.ellipsize(s, size, false, maxw),
        None => format!("{}\u{2026}", s),
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn light_text_edges_are_boosted_but_bounded() {
        let l = boost_lut();
        assert_eq!((l[0], l[255]), (0, 255));
        assert!(l.windows(2).all(|w| w[0] <= w[1]), "kurva harus menaik");
        assert!(l[64] > 64 && l[128] > 128, "tepi tengah harus lebih tebal");
        assert!(is_light([200, 203, 214]) && is_light([255, 255, 255]) && !is_light([20, 20, 28]));
    }

    use super::*;

    fn fnv(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    fn gfx_100() -> Gfx {
        Gfx::new(1.0)
    }

    fn alpha_at(g: &Gfx, x: u32, y: u32) -> u8 {
        g.pm.pixel(x, y).map_or(0, |p| p.alpha())
    }

    fn opaque_count(g: &Gfx) -> usize {
        g.pm.data().chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn canvas_and_boxes_are_physical() {
        let g = Gfx::new(1.5);
        assert_eq!((g.pm.width(), g.pm.height()), (810, 288));
        assert_eq!(g.dirty, (0, 0, 810, 288));
    }

    #[test]
    fn shapes_use_logical_coordinates() {
        let mut g = Gfx::new(2.0);
        g.rect(10.0, 10.0, 20.0, 20.0, &paint(255, 255, 255, 1.0));
        assert_eq!(alpha_at(&g, 25, 25), 255, "logis (12.5, 12.5) di dalam");
        assert_eq!(alpha_at(&g, 55, 55), 255, "logis (27.5, 27.5) di dalam");
        assert_eq!(alpha_at(&g, 65, 65), 0, "logis (32.5, 32.5) di luar");
    }

    #[test]
    fn text_is_measured_in_logical_px_and_drawn_scaled() {
        let mut g1 = Gfx::new(1.0);
        let mut g2 = Gfx::new(2.0);
        let (w1, w2) = (g1.width_ex("Avenged Sevenfold", 17.0, true), g2.width_ex("Avenged Sevenfold", 17.0, true));
        assert!((w1 - w2).abs() <= 1.0, "lebar logis harus hampir sama: {w1} vs {w2}");
        g2.txt("Hello", 100.0, 50.0, 13.0, [255, 255, 255], 1.0, 400.0);
        let mut xs = (u32::MAX, 0u32);
        for y in 0..g2.pm.height() {
            for x in 0..g2.pm.width() {
                if alpha_at(&g2, x, y) > 0 {
                    xs = (xs.0.min(x), xs.1.max(x));
                }
            }
        }
        assert!(xs.0 >= 196 && xs.0 <= 206, "teks mulai di x fisik ~200, dapat {}", xs.0);
        let w_phys = (xs.1 - xs.0) as f32;
        let w_log = g1.width("Hello", 13.0);
        assert!((w_phys / 2.0 - w_log).abs() <= 3.0, "lebar fisik {w_phys} harus ~2x lebar logis {w_log}");
    }

    #[test]
    fn whole_scene_scales_area_by_square_of_scale() {
        let mut g1 = Gfx::new(1.0);
        let mut g2 = Gfx::new(2.0);
        scene(&mut g1);
        scene(&mut g2);
        let (a1, a2) = (opaque_count(&g1) as f32, opaque_count(&g2) as f32);
        assert!((a2 / a1 - 4.0).abs() < 0.2, "luas 2x skala harus ~4x: {a1} -> {a2}");
    }

    /// Satu adegan yang memakai semua jalur gambar: bentuk, garis, ring, gradasi, ikon garis, teks biasa/tebal/terpotong.
    fn scene(g: &mut Gfx) {
        let shape = notch_path(EXP_W, EXP_H);
        g.fill(&shape, &paint(0, 0, 0, 1.0));
        crate::render::glow(g, &shape, EXP_H, (90, 120, 200), 0.4);
        g.fill(&rrect(40.5, 30.25, 120.0, 40.0, 12.0), &paint(90, 150, 255, 0.8));
        g.circle(300.0, 60.0, 14.5, &paint(255, 69, 58, 1.0));
        g.line(200.0, 100.0, 330.0, 140.0, 3.0, &paint(255, 255, 255, 0.6));
        g.ring(420.0, 80.0, 18.0, 0.62, 4.0, &paint(255, 255, 255, 0.2), &paint(90, 150, 255, 1.0));
        g.icon_bluetooth(470.0, 140.0, 16.0, &paint(255, 255, 255, 1.0));
        g.txt("Almost Easy · 15:19", 60.0, 90.0, 13.0, [255, 255, 255], 1.0, 400.0);
        g.txtb("Avenged Sevenfold", 60.0, 110.0, 17.0, [200, 220, 255], 0.9, 400.0);
        g.txt_clip("I'm not insane, I'm not insane", 50.0, 135.0, 12.0, [255, 255, 255], 1.0, 70.0, 220.0, 6.0);
    }

    #[test]
    fn thin_rect_straddling_two_pixels_does_not_panic() {
        // 10.6..11.9: touches two pixels without fully covering either (used to panic in debug builds)
        let mut g = gfx_100();
        g.rect(10.6, 4.0, 1.3, 7.0, &paint(255, 255, 255, 1.0));
        g.rect(4.0, 10.6, 7.0, 1.3, &paint(255, 255, 255, 1.0));
        assert!(alpha_at(&g, 10, 6) > 0 && alpha_at(&g, 11, 6) > 0);
    }

    #[test]
    fn scene_at_100_percent_is_unchanged() {
        let mut g = gfx_100();
        scene(&mut g);
        // Hash diperbarui sekali saat tepi teks terang diperkuat (boost_lut); sebelum itu 6528788650261307270 (= sebelum skala DPI).
        assert_eq!(fnv(g.pm.data()), 4458421239539419709, "hash adegan 100% berubah: pastikan perubahan gambar memang disengaja");
    }

    /// Implementasi lama (kuadratik) sebagai acuan kebenaran.
    fn ellipsize_slow(t: &mut Text, s: &str, size: f32, maxw: f32) -> String {
        let mut x = s.to_string();
        while !x.is_empty() && t.measure(&format!("{}\u{2026}", x), size, false) > maxw {
            x.pop();
        }
        x.push('\u{2026}');
        x
    }

    #[test]
    fn ellipsize_matches_slow_version_and_is_fast() {
        let mut t = Text::load().expect("font bawaan");
        let long = "Halo, apakah kamu masih ingat janji kita kemarin? Café résumé — \u{201c}kutipan\u{201d} dan lain-lain ".repeat(4);
        for s in ["", "a", "pendek", "Halo dunia yang cukup panjang untuk dipotong", "日本語のテストです。これは長い文章です", long.as_str()] {
            for w in [0.0f32, 10.0, 40.0, 120.0, 300.0, 5000.0] {
                assert_eq!(t.ellipsize(s, 12.0, false, w), ellipsize_slow(&mut t, s, 12.0, w), "s={s:?} w={w}");
            }
        }
        // karakter di luar font bawaan dirender lewat GDI: daftar font fontdue tidak boleh bertambah
        let before = (t.reg.len(), t.bold.len());
        let _ = t.measure("\u{201c}Halo\u{201d} \u{2014} \u{2713} 日本語 \u{1F600}", 13.0, false);
        let _ = t.measure("\u{201c}Halo\u{201d} \u{2014} \u{2713}", 13.0, true);
        assert_eq!((t.reg.len(), t.bold.len()), before, "font besar tidak boleh dimuat ke fontdue");
        // 8 baris x 200 frame (sekitar 3 detik animasi) harus tetap murah
        let t0 = std::time::Instant::now();
        for _ in 0..1600 {
            let _ = t.ellipsize(&long, 12.0, false, 300.0);
        }
        let fast = t0.elapsed();
        let t1 = std::time::Instant::now();
        for _ in 0..16 {
            let _ = ellipsize_slow(&mut t, &long, 12.0, 300.0);
        }
        // informasi: per panggilan, versi lama vs baru (jalankan dengan --nocapture)
        eprintln!("ellipsize 400 karakter: baru {:?} | lama {:?}", fast / 1600, t1.elapsed() / 16);
        assert!(fast.as_millis() < 400, "ellipsize terlalu lambat: {fast:?}");
    }
}

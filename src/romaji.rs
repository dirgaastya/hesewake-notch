//! Lirik Jepang -> romaji (dan Korea -> romanisasi lewat korean.rs, tanpa kamus). Lindera (kamus IPADIC dari berkas, diunduh lewat tombol di Settings, lihat dict.rs; tidak
//! ditanam di exe) memberi bacaan
//! katakana per kata, lalu `wana_kana` mengubahnya ke romaji. Dikerjakan sekali per lagu di thread lirik; kamus
//! (±47 MB) dimuat hanya bila lirik berisi kana dan langsung dilepas lagi setelah konversi.

use super::*;
use lindera::dictionary::{load_dictionary, Dictionary};
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lyrics::LyricLine;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use wana_kana::ConvertJapanese;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Settings > Music & lyrics > "Romanize lyrics".
pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Folder kamus: env `HESEWAKE_DICT`, hasil unduhan (%APPDATA%\hesewake-notch\dict\ipadic), `<folder exe>\dict\ipadic`,
/// lalu folder induk exe (`cargo run` dari `target\release`, `cargo test` dari `target\debug\deps`, atau
/// `target\<triple>\...` dengan `--target`; isi dengan scripts\fetch-dict.ps1). Hanya hasil yang ditemukan di-cache,
/// supaya kamus yang baru diunduh langsung terpakai; `forget_dir` mengosongkannya setelah kamus dihapus.
fn dict_dir() -> Option<PathBuf> {
    let mut cache = DIR.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(d) = cache.as_ref() {
        return Some(d.clone());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("HESEWAKE_DICT").map(PathBuf::from).into_iter().collect();
    dirs.push(dict::user_dir());
    if let Some(exe) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        dirs.extend(exe.ancestors().take(5).map(|d| d.join("dict").join("ipadic")));
    }
    let found = dirs.into_iter().find(|d| d.join("metadata.json").is_file())?;
    *cache = Some(found.clone());
    Some(found)
}

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Folder kamus yang dipakai sekarang (untuk tombol Delete di Settings).
pub(crate) fn active_dir() -> Option<PathBuf> {
    dict_dir()
}

/// Lupakan lokasi kamus yang di-cache (dipanggil setelah kamus hasil unduhan dihapus).
pub(crate) fn forget_dir() {
    *DIR.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Kamus terpasang (untuk label Settings).
pub(crate) fn available() -> bool {
    dict_dir().is_some()
}

/// Baris ini bisa diberi romanisasi: kana (Jepang) atau Hangul (Korea).
pub(crate) fn needs_roma(s: &str) -> bool {
    has_kana(s) || korean::has_hangul(s)
}

/// Ada hiragana/katakana. Lirik tanpa kana (Mandarin: kanji saja) tidak dikonversi dengan bacaan Jepang.
pub(crate) fn has_kana(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3041..=0x309F | 0x30A1..=0x30FA))
}

/// Isi `roma` untuk tiap baris Jepang. Blocking (memuat kamus): panggil dari thread lirik.
pub(crate) fn annotate(lines: &mut [LyricLine]) {
    if !enabled() {
        return;
    }
    // Korea tidak butuh kamus.
    korean::annotate(lines);
    if !lines.iter().any(|l| has_kana(&l.text)) {
        return;
    }
    let Some(dir) = dict_dir() else { return };
    let t0 = Instant::now();
    let dict = match load_dictionary(&dir.to_string_lossy()) {
        Ok(d) => d,
        Err(e) => {
            perf::note(&format!("romaji: kamus gagal dimuat dari {}: {:?}", dir.display(), e));
            return;
        }
    };
    let seg = Segmenter::new(Mode::Normal, dict, None);
    let t1 = Instant::now();
    let mut n = 0;
    for l in lines.iter_mut().filter(|l| l.roma.is_empty() && l.text.chars().any(cjk::is_cjk)) {
        l.roma = line(&seg, &l.text);
        n += 1;
    }
    perf::note(&format!(
        "romaji: muat kamus {} ms, {} baris {} ms, mem {:.1} MB",
        (t1 - t0).as_millis(),
        n,
        t1.elapsed().as_millis(),
        perf::mem_mb()
    ));
    // `seg` (beserta kamus) dilepas di sini.
}

fn line(seg: &Segmenter, text: &str) -> String {
    // IPADIC memecah あなたがた jadi あなた+が+たと (kata kerja); 方 membacanya "gata".
    let text = text.replace("あなたがた", "あなた方");
    let Ok(mut tokens) = seg.segment(Cow::Borrowed(&text)) else { return String::new() };
    let toks: Vec<Tok> = tokens
        .iter_mut()
        .map(|t| {
            let surface = t.surface.to_string();
            let dict = t.dictionary;
            let mut d = t.details();
            if d.get(1) == Some(&"固有名詞") && d.get(2) == Some(&"人名") && surface.chars().all(is_kanji) {
                if let Some(alt) = common_reading(dict, &surface) {
                    d = alt;
                }
            }
            (surface, d.iter().map(|x| x.to_string()).collect())
        })
        .collect();
    join(&pieces(&toks))
}

/// Token: permukaan dan detail IPADIC (sudah melewati koreksi nama).
type Tok = (String, Vec<String>);

/// Bacaan yang salah di IPADIC untuk kata umum di lirik, dikoreksi dari konteks token sekitarnya.
/// Mengembalikan (bacaan kana, jumlah token yang dipakai, ditempel ke token sebelumnya).
fn fix_reading(toks: &[Tok], i: usize) -> Option<(&'static str, usize, bool)> {
    let (surface, d) = (toks[i].0.as_str(), &toks[i].1);
    let prev = i.checked_sub(1).map(|j| &toks[j]);
    let prev_name = prev.map_or(false, |t| t.1.get(1).map(String::as_str) == Some("固有名詞"));
    let next = toks.get(i + 1);
    let next_surface = next.map(|t| t.0.as_str());
    let next_pos = next.and_then(|t| t.1.first()).map(String::as_str);
    match (surface, next_surface) {
        // 二 + 人: IPADIC membaca "ninin"; 人 setelah angka 2 selalu "ふたり".
        ("二", Some("人")) => Some(("フタリ", 2, false)),
        ("体", Some("中")) => Some(("カラダジュウ", 2, false)),
        // 角 sebelum partikel: "かど" (sudut jalan), bukan "かく".
        ("角", _) if next_pos == Some("助詞") => Some(("カド", 1, false)),
        // 君 yang berdiri sendiri (bukan akhiran nama, mis. 田中君): "きみ", bukan "くん".
        ("君", _) if d.get(1).map(String::as_str) == Some("接尾") && !prev_name => Some(("キミ", 1, false)),
        // あなた + 方: "あなたがた" (jamak), bukan "かた".
        ("方", _) if prev.map(|t| t.0.as_str()) == Some("あなた") && d.get(1).map(String::as_str) == Some("接尾") => Some(("ガタ", 1, true)),
        _ => None,
    }
}

/// Token -> potongan, dengan koreksi bacaan (`fix_reading`).
fn pieces(toks: &[Tok]) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Some((kana, used, glue)) = fix_reading(toks, i) {
            out.push(Piece { text: kana.to_string(), glue });
            i += used;
            continue;
        }
        let d: Vec<&str> = toks[i].1.iter().map(String::as_str).collect();
        let mut pc = piece(&toks[i].0, &d);
        // Kata kerja majemuk: 連用形 + 合う/合える (分かち合う, 伝え合える) ditulis menyatu.
        let after_stem = i > 0 && toks[i - 1].1.first().map(String::as_str) == Some("動詞") && toks[i - 1].1.get(5).map(String::as_str) == Some("連用形");
        if after_stem && d.first() == Some(&"動詞") && d.get(6).map_or(false, |b| b.starts_with('合')) {
            pc.glue = true;
        }
        out.push(pc);
        i += 1;
    }
    out
}

/// Lirik jarang berisi nama orang dalam kanji, tapi IPADIC kadang memilih entri nama (次の章 -> "akira", bukan
/// "shou"). Ambil entri lain dengan permukaan yang sama yang bukan nama diri dan punya bacaan, dengan biaya kata
/// terendah. None bila kata itu memang hanya ada sebagai nama.
fn common_reading<'a>(dict: &'a Dictionary, surface: &str) -> Option<Vec<&'a str>> {
    dict.prefix_dictionary
        .find_surface(surface)
        .into_iter()
        .filter(|e| e.word_id().is_system())
        .map(|e| (e.word_cost(), dict.word_details(e.word_id().id() as usize)))
        .filter(|(_, d)| d.len() >= 9 && d[1] != "固有名詞" && d[7] != "*")
        .min_by_key(|(cost, _)| *cost)
        .map(|(_, d)| d)
}

fn is_kanji(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3005)
}

/// Satu token: kana (bacaan) atau teks apa adanya, dan apakah ditempel ke token sebelumnya tanpa spasi.
#[derive(Debug, PartialEq)]
struct Piece {
    text: String,
    glue: bool,
}

/// Token -> potongan. `d` = detail IPADIC: `[0]` kelas kata, `[1]` sub-kelas, `[6]` bentuk dasar, `[7]` bacaan,
/// `[8]` pelafalan. Token tak dikenal hanya punya 7 kolom (tanpa bacaan).
fn piece(surface: &str, d: &[&str]) -> Piece {
    let at = |i: usize| d.get(i).copied().unwrap_or("*");
    let (pos, sub) = (at(0), at(1));
    if surface.trim().is_empty() {
        return Piece { text: String::new(), glue: false };
    }
    if !surface.chars().any(char::is_alphanumeric) {
        return Piece { text: punct(surface), glue: true };
    }
    // Ditempel: kata kerja bantu (ます, た, ない; bukan です/だ kopula, tetapi だ bentuk lampau: 読んだ),
    // akhiran (さん, れる), て/で/ば penghubung, dan singkatan ている (てる/でる).
    let glue = (pos == "助動詞" && (!matches!(at(6), "です" | "だ") || at(4) == "特殊・タ"))
        || sub == "接尾"
        || (sub == "接続助詞" && matches!(surface, "て" | "で" | "ば"))
        || (pos == "動詞" && sub == "非自立" && matches!(at(6), "てる" | "でる"));
    let text = if at(7) != "*" {
        // Partikel memakai pelafalan: は -> ワ, へ -> エ, を -> オ.
        (if pos == "助詞" && at(8) != "*" { at(8) } else { at(7) }).to_string()
    } else {
        halfwidth(surface)
    };
    Piece { text, glue }
}

/// Gabungkan potongan per kelompok (kana kelompok dirangkai dulu supaya っ dan ー di batas token benar, mis.
/// 行っ + た -> "itta"), lalu ubah ke romaji.
fn join(pieces: &[Piece]) -> String {
    let mut groups: Vec<String> = Vec::new();
    for p in pieces {
        match groups.last_mut() {
            Some(g) if p.glue => g.push_str(&p.text),
            _ if p.text.is_empty() => groups.push(String::new()),
            _ => groups.push(p.text.clone()),
        }
    }
    let words: Vec<String> = groups.iter().filter(|g| !g.is_empty()).map(|g| g.as_str().to_romaji()).collect();
    words.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tanda baca Jepang -> ASCII; kurung/kutip siku dibuang.
fn punct(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '、' | '，' => Some(','),
            '。' | '．' => Some('.'),
            '「' | '」' | '『' | '』' | '【' | '】' => None,
            '・' | '\u{3000}' => Some(' '),
            '〜' => Some('~'),
            c => Some(fullwidth_to_ascii(c)),
        })
        .collect()
}

fn halfwidth(s: &str) -> String {
    s.chars().map(fullwidth_to_ascii).collect()
}

/// Huruf/angka/tanda baca lebar penuh (U+FF01..FF5E) -> ASCII.
fn fullwidth_to_ascii(c: char) -> char {
    match c as u32 {
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(surface: &str, d: &str) -> Piece {
        piece(surface, &d.split(',').collect::<Vec<_>>())
    }

    #[test]
    fn kana_detection() {
        assert!(has_kana("夜に駆ける") && has_kana("カタカナ"));
        assert!(!has_kana("我爱你") && !has_kana("hello") && !has_kana("사랑해") && !has_kana("ー・"));
    }

    #[test]
    fn particles_use_pronunciation_and_auxiliaries_glue() {
        let s = join(&[
            p("私", "名詞,代名詞,一般,*,*,*,私,ワタシ,ワタシ"),
            p("は", "助詞,係助詞,*,*,*,*,は,ハ,ワ"),
            p("東京", "名詞,固有名詞,地域,一般,*,*,東京,トウキョウ,トーキョー"),
            p("へ", "助詞,格助詞,一般,*,*,*,へ,ヘ,エ"),
            p("行き", "動詞,自立,*,*,五段・カ行促音便,連用形,行く,イキ,イキ"),
            p("ます", "助動詞,*,*,*,特殊・マス,基本形,ます,マス,マス"),
            p("。", "記号,句点,*,*,*,*,。,。,。"),
        ]);
        assert_eq!(s, "watashi wa toukyou e ikimasu.");
    }

    #[test]
    fn sokuon_and_te_join_across_tokens() {
        let s = join(&[
            p("行っ", "動詞,自立,*,*,五段・カ行促音便,連用タ接続,行く,イッ,イッ"),
            p("た", "助動詞,*,*,*,特殊・タ,基本形,た,タ,タ"),
            p("見", "動詞,自立,*,*,一段,連用形,見る,ミ,ミ"),
            p("て", "助詞,接続助詞,*,*,*,*,て,テ,テ"),
        ]);
        assert_eq!(s, "itta mite");
    }

    #[test]
    fn unknown_words_punctuation_and_spaces() {
        let s = join(&[
            p("「", "記号,括弧開,*,*,*,*,「,「,「"),
            p("ＯＫ", "名詞,一般,*,*,*,*,*"),
            p("」", "記号,括弧閉,*,*,*,*,」,」,」"),
            p("\u{3000}", "記号,空白,*,*,*,*,\u{3000},\u{3000},\u{3000}"),
            p("きれい", "名詞,形容動詞語幹,*,*,*,*,きれい,キレイ,キレイ"),
            p("です", "助動詞,*,*,*,特殊・デス,基本形,です,デス,デス"),
            p("ね", "助詞,終助詞,*,*,*,*,ね,ネ,ネ"),
            p("、", "記号,読点,*,*,*,*,、,、,、"),
            p("Baby", "名詞,固有名詞,組織,*,*,*,*"),
        ]);
        assert_eq!(s, "OK kirei desu ne, Baby");
    }

    #[test]
    fn long_vowel_mark_in_katakana_reading() {
        assert_eq!(join(&[p("スカイツリー", "名詞,固有名詞,一般,*,*,*,スカイツリー,スカイツリー,スカイツリー")]), "sukaitsurii");
    }

    #[test]
    fn end_to_end_with_dictionary() {
        // butuh kamus (scripts\fetch-dict.ps1); dilewati bila tidak ada
        let Some(dir) = dict_dir() else { return };
        let seg = Segmenter::new(Mode::Normal, load_dictionary(&dir.to_string_lossy()).expect("kamus"), None);
        assert_eq!(line(&seg, "私は東京へ行きます"), "watashi wa toukyou e ikimasu");
        assert_eq!(line(&seg, "夜に駆ける"), "yoru ni kakeru");
        // 章 dipilih sebagai nama orang (アキラ) oleh IPADIC; harus memakai bacaan kata biasa
        assert!(line(&seg, "めくれば次の章").ends_with("tsugi no shou"));
        set_enabled(true);
        let mut v = vec![
            LyricLine { at_ms: 0, text: "Hello".into(), roma: String::new() },
            LyricLine { at_ms: 1, text: "君の名は".into(), roma: String::new() },
        ];
        annotate(&mut v);
        assert!(v[0].roma.is_empty());
        assert_eq!(v[1].roma, "kimi no na wa");
    }

    fn t(surface: &str, d: &str) -> Tok {
        (surface.to_string(), d.split(',').map(str::to_string).collect())
    }

    #[test]
    fn past_da_and_contracted_teiru_glue() {
        let s = join(&pieces(&[
            t("くすん", "動詞,自立,*,*,五段・マ行,連用タ接続,くすむ,クスン,クスン"),
            t("だ", "助動詞,*,*,*,特殊・タ,基本形,だ,ダ,ダ"),
        ]));
        assert_eq!(s, "kusunda");
        let s = join(&pieces(&[
            t("笑っ", "動詞,自立,*,*,五段・ワ行促音便,連用タ接続,笑う,ワラッ,ワラッ"),
            t("てる", "動詞,非自立,*,*,一段,基本形,てる,テル,テル"),
        ]));
        assert_eq!(s, "waratteru");
        let s = join(&pieces(&[
            t("分かち", "動詞,自立,*,*,五段・タ行,連用形,分かつ,ワカチ,ワカチ"),
            t("合う", "動詞,非自立,*,*,五段・ワ行促音便,基本形,合う,アウ,アウ"),
        ]));
        assert_eq!(s, "wakachiau");
        // だ kopula (特殊・ダ) tetap terpisah
        let s = join(&pieces(&[
            t("雨", "名詞,一般,*,*,*,*,雨,アメ,アメ"),
            t("だ", "助動詞,*,*,*,特殊・ダ,基本形,だ,ダ,ダ"),
        ]));
        assert_eq!(s, "ame da");
    }

    #[test]
    fn known_misreadings_are_corrected() {
        let nara = t("なら", "助動詞,*,*,*,特殊・ダ,仮定形,だ,ナラ,ナラ");
        let s = join(&pieces(&[t("二", "名詞,数,*,*,*,*,二,ニ,ニ"), t("人", "名詞,接尾,助数詞,*,*,*,人,ニン,ニン"), nara]));
        assert_eq!(s, "futari nara");
        let s = join(&pieces(&[
            t("耳", "名詞,一般,*,*,*,*,耳,ミミ,ミミ"),
            t("を", "助詞,格助詞,一般,*,*,*,を,ヲ,ヲ"),
            t("伝い", "動詞,自立,*,*,五段・ワ行促音便,連用形,伝う,ツタイ,ツタイ"),
            t("体", "名詞,接尾,一般,*,*,*,体,タイ,タイ"),
            t("中", "名詞,接尾,副詞可能,*,*,*,中,チュウ,チュー"),
        ]));
        assert_eq!(s, "mimi wo tsutai karadajuu");
        let s = join(&pieces(&[
            t("角", "名詞,一般,*,*,*,*,角,カク,カク"),
            t("を", "助詞,格助詞,一般,*,*,*,を,ヲ,ヲ"),
            t("曲がれ", "動詞,自立,*,*,五段・ラ行,仮定形,曲がる,マガレ,マガレ"),
        ]));
        assert_eq!(s, "kado wo magare");
        // 角 sebagai kata biasa (tanpa partikel) tidak diubah
        assert_eq!(join(&pieces(&[t("角", "名詞,一般,*,*,*,*,角,カク,カク")])), "kaku");
    }

    #[test]
    fn kun_is_kimi_unless_after_a_name() {
        let kun = t("君", "名詞,接尾,人名,*,*,*,君,クン,クン");
        let ni = t("に", "助詞,格助詞,一般,*,*,*,に,ニ,ニ");
        let ichido = t("いちど", "名詞,一般,*,*,*,*,いちど,イチド,イチド");
        assert_eq!(join(&pieces(&[ichido, kun.clone(), ni])), "ichido kimi ni");
        let tanaka = t("田中", "名詞,固有名詞,人名,姓,*,*,田中,タナカ,タナカ");
        assert_eq!(join(&pieces(&[tanaka, kun])), "tanakakun");
    }
}

# Hesewake Notch: panduan kerja untuk Claude Code

Dokumen ini ditulis supaya sesi baru langsung paham proyek dan cara kerja kita, tanpa mengulang percakapan lama. Baca
sampai habis sebelum mengerjakan apa pun. Hal-hal khusus edisi Office (kantor) ada di `CLAUDE.local.md` (tidak masuk git).

## 1. Proyek

- **Hesewake Notch**: notch untuk tepi atas layar Windows (musik, jam, air minum, to-do, Ledge, Scrap, notifikasi, timer, kalender,
  waktu sholat, dan lain-lain). Rust + Win32, render software (`tiny-skia`, `fontdue`), jendela layered, kanvas 540×192
  logis (skala DPI lewat `scale::current()`), animasi pegas. Fork pribadi dari [Poch](https://github.com/klawcodes/poch)
  (MIT). Lisensi proyek **MIT**: jangan menyalin kode GPL/AGPL (mis. BitChord, GPLv3). Belajar dari ide boleh, menyalin kode tidak.
- Repo: `https://github.com/dirgaastya/hesewake-notch` (**publik**), remote `origin`, `gh` sudah login sebagai `dirgaastya`.
  Dari luar folder repo, `gh` butuh `-R dirgaastya/hesewake-notch`.
- Folder kerja: `C:\Users\ThinkPad\Desktop\project\personal\notch\hesewake-notch` (git Bash dan PowerShell tersedia).
- Pemilik (dirga) berbicara Indonesia. **Balas selalu dalam bahasa Indonesia**, istilah teknis dan identifier tetap bahasa asli.
  Teks untuk kontributor di GitHub (komentar PR/issue, deskripsi PR, balasan di issue) ditulis **bahasa Inggris**.
  Entri `CHANGELOG.md` ditulis bahasa Indonesia.
- Edisi: **reguler** (publik, cabang `main`) dan **Office** (kantor, cabang lokal `office`, tidak pernah dipush; lihat `CLAUDE.local.md`).

## 2. Build, tes, dan alat

| Perlu | Perintah |
|---|---|
| Tes lengkap (seperti CI) | `RUSTFLAGS="-C target-feature=+crt-static -D warnings" cargo test --release` |
| Satu tes | `cargo test --release <nama>` (hanya **satu** filter per perintah) |
| Build | `RUSTFLAGS="-C target-feature=+crt-static" cargo build --release` → `target\release\hesewake-notch.exe` |
| Tes jaringan (di-`#[ignore]`) | `cargo test --release <nama> -- --ignored` |
| Installer | `powershell -File scripts/build-installer.ps1` (Inno Setup; `-Office` untuk Office) |
| Snapshot UI sebagai PNG | `SNAPSHOT_DIR=<folder> cargo test --release <nama_tes>` lalu lihat PNG-nya dengan alat Read |

- **Empat tes gagal di PC ini bila baru restart** (uptime < 3 hari): `clipboard_click_copies_back_and_moves_to_top`,
  `clipboard_remove_and_clear_buttons`, `snap_clipboard`, `snap_indonesian`. Penyebabnya `Instant::now() - Duration::from_secs(3 * 86400)`
  yang panik bila mesin baru menyala; **bukan bug aplikasi**. CI (runner baru) lulus, jadi CI yang menentukan. Jangan "memperbaiki" ini
  tanpa diminta; sebutkan di laporan bahwa hanya keempat itu yang gagal.
- Selalu lihat hasil visual lewat snapshot untuk perubahan tampilan (render PNG, lalu `Read` gambarnya). Tes snapshot juga
  memeriksa teks muat di sel dan stepper, dua bahasa (EN dan ID).
- `Cargo.lock` dan `Cargo.toml` selalu dinaikkan bersama saat rilis.
- File sumber memakai **CRLF**. Saat mengedit lewat skrip Python, baca dengan `newline=''`, ubah ke `\n`, lalu kembalikan ke CRLF
  (lihat pola `edit()` di skrip lama). Edit kecil lewat alat Edit biasanya aman.

## 3. Arsitektur singkat (hal yang sering disentuh)

- Semua modul `use super::*;`. State aplikasi satu struct `App` (`main.rs`) di `static APP: Mutex<Option<App>>`, diakses lewat
  `with_app(|a| ...)`. **Fixture tes `test_app` (`snapshot.rs`) harus diisi setiap field baru `App`.**
- **Slide**: `slides.rs` (`SlideId`, `USER`, `available()`, `effective()`, urutan dan tampil/sembunyi di `settings.txt` kunci `slides=`).
  Slide opsional baru tersembunyi secara default kecuali memang dimaksudkan nyala. Hotkey slide `Ctrl+Alt+1..` mengikuti urutan.
- **Settings** (`settings.rs` untuk simpan/parse, `settingsview.rs` untuk tampilan): tab **Slides, General, Alerts, Widgets**.
  Tiap baris punya `Rid` (id); klik dikenali lewat `match r.id`, **bukan nomor urut**. Menambah pengaturan = tambah `Rid`, baris di
  `settings_rows(tab)`, arm klik di `settings_click_rows`, field + parse + serialize + tes roundtrip di `settings.rs`, teks di `i18n.rs`.
  Tab baris menampilkan 3 baris sel (2 kolom, kiri ke kanan) dan sisanya di-scroll dengan roda. Label dan nilai stepper harus muat
  di sel (tes `settings_text_fits_cells`, `stepper_values_fit_without_cutting` akan gagal bila terlalu panjang; pendekkan teksnya).
- **i18n** (`i18n.rs`): `tr("English")` mengembalikan teks bahasa aktif; teks Inggris adalah kuncinya. Satu tes memindai semua
  `tr("...")` di `src/` dan gagal bila ada terjemahan hilang, tak terpakai, ganda, atau placeholder tak cocok. Nama produk dan fitur
  (Pomodoro, Timer, Ledge, Scrap, HUD) tidak diterjemahkan. Teks bernilai dinamis memakai `fill(tr("{} min"), &[&5])`.
- **Toast** (`toast.rs`): `ToastKind`. Toast yang punya tombol (Water, rapat, pembaruan, unduhan) harus masuk `toast_has_button()`
  supaya hover tidak membatalkannya sebelum klik.
- **Hover / minimal / menyingkir** (`anim.rs::pointer_step`): di atas jendela maksimal atau layar penuh (`fg_state`) notch menciut jadi
  garis tipis saat kursor mendekat dan tidak membuka slide; kursor tepat di garis tipis (± 5 px dari tepi atas) membuka slide.
  Peek (hotkey, notifikasi) dan lirik yang disematkan (`pinned`) tidak terganggu. Tidak ada pengaturannya (sengaja global).
- **Jendela topmost** bisa tenggelam di bawah jendela biasa walau penandanya masih ada: pemeriksaan tiap detik di `anim.rs` juga
  memeriksa z-order (`buried`). Jika notch "hilang tapi proses hidup", curigai ini dulu (bandingkan `PrintWindow` vs layar).
- **Pesan jendela** memakai `WM_APP + n`. Sebelum menambah, `grep -rn "WM_APP + " src/` supaya tidak bentrok. Terpakai: 1 tray, 2 bt,
  3 notif, 4 lockkey, 5 privacy, 6 volume, 7 brightness, 8 powercheck, 9 display, 10 weather, 11 calendar, 12 update, 13 mic,
  14 update download, 15 output, 16 downloads, 17 adhan file. (Edisi Office memakai 40.)
- **Lirik** (`lyrics.rs`): LRCLIB dulu, lalu **lrc.red** bila LRCLIB tak punya lirik sinkron. Syarat lrc.red: User-Agent berisi nama aplikasi,
  tautan repo, dan email kontak (`hesewake@gmail.com`), serta kredit sumber ("via lrc.red", klik membuka situsnya) di mode lirik dan About.
  Unison sudah dicoba dan dilewati (tidak menemukan lagu tanpa ID video YouTube). Romanisasi: Jepang lewat Lindera (kamus 47 MB diunduh
  atas permintaan, `dict.rs`), Korea lewat `korean.rs` (Revised Romanization, tanpa kamus). Mandarin (pinyin) belum ada, ditunda.
- **Pembaruan** (`update.rs`): cek `releases/latest` saat start dan tiap 24 jam. Tombol **Install** mengunduh installer dari
  `github.com/dirgaastya/hesewake-notch/releases/download/`, memeriksa ukuran dan SHA-256 terhadap `SHA256SUMS` di rilis yang sama, lalu menjalankan
  installer biasa (tidak senyap). Rilis tanpa `SHA256SUMS` hanya membuka halaman rilis.
- Data pengguna di `%APPDATA%\hesewake-notch\` (settings.txt, dll). **Jangan menimpa data pengguna saat uji**; tes tidak boleh menulis
  (`Settings::save` dilewati saat `cfg!(test)`).

## 4. Alur rilis (urutan tetap)

Versi: `0.MINOR.PATCH`. Satu rilis = satu cabang rilis → satu PR → merge commit → tag → draf rilis → verifikasi → publikasi.

1. `git fetch origin`, buat cabang dari `origin/main` (`release/X.Y.Z` untuk gabungan PR kontributor, atau cabang fitur/perbaikan).
2. Gabungkan PR kontributor dengan `git merge --no-ff --no-edit pr/N` (setelah `git fetch origin '+refs/pull/N/head:pr/N'`).
   **Sebelum merge**, pastikan head PR di GitHub sama dengan yang diambil (`gh pr view N --json headRefOid` vs `git rev-parse pr/N`):
   kontributor kadang menambah commit setelah kita mengambil. Selesaikan konflik dengan **mempertahankan semua sisi** (jangan membuang fitur
   siapa pun). Tes tulisan interleaved di `snapshot.rs` sering konflik: pisahkan menjadi fungsi tes terpisah.
3. Naikkan versi (`Cargo.toml`, `Cargo.lock`), tulis entri `## [X.Y.Z] - tanggal` di `CHANGELOG.md` (Indonesia, kredit kontributor dengan `@nama` dan nomor PR;
   workflow rilis mengambil catatan dari heading ini), jalankan tes lengkap, commit.
4. `git push -u origin <cabang>`, `gh pr create` (isi PR bahasa Inggris, jelaskan apa yang ditambahkan di atas PR kontributor), tunggu CI hijau (pakai Monitor, lihat §7).
5. Untuk perubahan yang terlihat pengguna: bangun exe, jalankan sebagai **trial** untuk dicoba pemilik (lihat §6), lalu **tanya dulu**.
6. Setelah pemilik menulis **"merge #N"**: `gh pr merge N -R dirgaastya/hesewake-notch --merge` (merge commit, **bukan squash**, supaya commit kontributor tetap atas
   nama mereka dan PR mereka berstatus Merged). Pastikan PR kontributor berstatus MERGED; beri komentar terima kasih singkat (Inggris) di tiap PR.
7. `git fetch origin main && git tag -a vX.Y.Z -m "X.Y.Z" origin/main && git push origin vX.Y.Z` → workflow **Release** membangun installer dan membuat draf
   rilis berisi `HesewakeNotch-Setup-X.Y.Z.exe` dan `SHA256SUMS`.
8. **Verifikasi sebelum publikasi**: unduh kedua berkas draf (`gh release download vX.Y.Z -p ...`), bandingkan `sha256sum` installer dengan `SHA256SUMS`.
   Lalu `gh release edit vX.Y.Z --draft=false --latest`. Setelah itu `curl https://api.github.com/repos/dirgaastya/hesewake-notch/releases/latest`
   harus menampilkan kedua aset (fitur Install bergantung padanya).
9. Sinkron ke Office (lihat `CLAUDE.local.md`), bangun installer Office (lokal, tidak dipublikasikan).

Rilis yang sudah keluar: 0.1.0 … 0.11.0 (terakhir publik: **0.11.0**). Rilis 0.11.1 ada di PR #44.

## 5. Kapan harus bertanya (aturan pemilik: "tanyain ulang yang perlu dikonfirmasi")

**Selalu minta persetujuan eksplisit** sebelum: merge PR (harness menolak merge tanpa kata "merge #N" dari pemilik), menandai atau mempublikasikan rilis,
menghapus cabang/tag di remote, menutup aplikasi pemilik yang sedang berjalan (mis. edisi Office), memosting ke GitHub atas nama pemilik hal yang
belum pernah disetujui (balasan issue berisi keputusan baru), dan apa pun yang memengaruhi pihak luar. Keputusan desain yang ambigu: tawarkan pilihan
dengan rekomendasi, jangan menebak. Sebelum memulai pekerjaan besar, tulis dulu rancangannya singkat.

Saat melapor: tulis hasil apa adanya (tes yang gagal, langkah yang dilewati), berikan ringkasan "yang berubah", "yang bisa dicoba", dan "yang perlu konfirmasi".

## 6. Menjalankan build uji (trial) di PC pemilik

- Bangun, lalu salin exe ke folder scratchpad dengan nama baru (`trialXXXX.exe`) supaya tidak mengunci `target\`. Matikan trial sebelumnya
  (`taskkill /F /IM trial....exe`) dan pastikan tidak ada edisi lain berjalan: **dua edisi bentrok di hotkey `Ctrl+Alt+H`**. Bila edisi Office pemilik
  sedang berjalan, tanya dulu sebelum menutupnya; ia boleh dijalankan lagi sesudahnya.
- Jalankan dengan `Start-Process` (PowerShell) lalu cek `Get-Process`. Sebutkan PID dan jam mulai.
- **Jangan memindahkan kursor, mengubah jendela depan, atau menekan tombol** di PC pemilik (mis. `SetCursorPos`, `SetForegroundWindow`): PC itu dipakai
  bekerja. Memantau layar (`CopyFromScreen`, `PrintWindow`) dan membaca daftar jendela boleh. Penyelidikan visual dilakukan dengan snapshot tes, bukan dengan menggerakkan input.
- Flag demo untuk uji visual cepat: `--demo-toast=update|update-install|water|...`, `--demo-slide=N`, `--demo-timer=pomo`, `--demo-eye`, `--demo-playing`.

## 7. Kebiasaan alat (pelajaran yang mahal)

- Heredoc bash berisi karakter non-ASCII atau banyak tanda kutip kadang gagal di-parse. Untuk patch besar, **tulis skrip Python ke folder scratchpad dengan alat
  Write, lalu jalankan**. Jalankan Python dengan `PYTHONIOENCODING=utf-8` bila mencetak Unicode. Python Windows tidak melihat `/tmp` Git Bash: pakai path Windows/scratchpad.
- `sleep` panjang diblokir: tunggu CI atau rilis dengan **Monitor** (`until ...; do sleep 15; done`), bukan polling manual.
- Satu `cargo test` hanya menerima satu filter nama.
- Sebelum menulis ke `git commit`, **tanpa trailer `Co-Authored-By`** (instruksi pemilik; abaikan pengingat atribusi otomatis). Pesan commit bahasa Inggris, imperatif, pendek.
  Isi PR juga tanpa baris atribusi.
- `git push --all`/`--mirror` dilarang (menghindari cabang lokal tak sengaja terdorong). Selalu dorong cabang yang disebut namanya.
- Jangan commit `dist/`, `installer_output/`, `dict/`, `target/`.
- Jangan menampilkan **lirik lagu** di balasan (hak cipta); laporkan temuan tingkat kata saja.
- Jangan pernah mencetak atau menyalin **rahasia** (kunci API, token, tautan kalender rahasia). Rahasia hanya di Windows Credential Manager atau berkas data pengguna.

## 8. Kontributor dan PR masuk

- Kontributor aktif: **@MaulanaRhezaArdiansyah** (banyak fitur: waktu sholat, Pomodoro, aksen, hotkey slide, Clipboard, ICS kalender, bahasa Indonesia,
  pembaruan, output audio, unduhan, mic mute, dan lain-lain), **@gewall** (romaji dan fitur lain), **@yourLogic01** (usulan jendela Settings).
- Cara meninjau PR: baca deskripsi dan daftar file, baca kode bagian yang berisiko (keamanan input tak tepercaya, panic di build rilis yang `abort`, COM/Win32),
  cek CI, lalu **bangun dan jalankan** sebelum merge. Laporkan temuan ke pemilik dengan rekomendasi. Balas issue dengan keputusan pemilik (bahasa Inggris).
- Kontributor sudah diminta menambah baris Settings memakai `Rid` di tab yang tepat dan mengikuti pola yang ada.

## 9. Status terakhir (31 Oktober 2026)

- Publik terakhir: **0.11.0**. **PR #44 (0.11.1)** menunggu "merge #44": jam kecil di slide Music (menggantikan blok jam/hari/tanggal), pin mode lirik (tombol),
  notch menyingkir dari kursor di jendela maksimal/layar penuh (garis tipis membuka slide), perbaikan notch yang tertutup (z-order).
- PR terbuka belum ditinjau: **#45** idle clock ikon cuaca (Maulana), **#47** Wibu mode (gewall), **#49** Notch lock (gewall; tumpang tindih dengan pin dan menyingkir milik #44), **#50** Water ikut aksen (Maulana; menutup #41),
  **#51** jendela Settings terpisah (yourLogic01; besar, +2404/-1269, menyentuh Settings: gabungkan PR lain yang menambah baris Settings lebih dulu). Issue #34 (Settings jadi jendela) sudah dijawab pemilik: setuju, slide Settings dihapus di PR yang sama.
- Rencana **0.12.0**: edisi Office diubah menjadi extension (lihat `CLAUDE.local.md`).
- Keputusan yang masih menunggu pemilik: tidak ada selain "merge #44" dan prioritas peninjauan PR baru.

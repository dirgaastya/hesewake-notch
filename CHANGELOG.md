# Changelog

Format mengikuti [Keep a Changelog](https://keepachangelog.com/), versi mengikuti [Semantic Versioning](https://semver.org/).

## [0.11.1] - 2026-10-30

### Added
- **Sematkan mode lirik**: di mode lirik (ikon balon di slide Music) ada tombol pin di pojok kanan atas panel kiri. Saat disematkan (pin biru), notch **tetap terbuka** walau pointer pergi, jadi lirik terus terlihat. Klik pin lagi untuk melepas. Penyematan hanya berlaku selama mode lirik menyala, dan keluar dari mode lirik otomatis melepasnya. Di atas aplikasi layar penuh (mis. video), notch tetap menyusut seperti biasa.

### Changed
- **Slide Music tanpa blok jam, hari, dan tanggal** di sisi kanan: teks lagu, progres, dan kontrol kini memakai seluruh lebar slide. Sebagai gantinya ada **jam kecil** (HH:MM) di pojok kanan atas, di baris yang sama dengan "Now Playing"; baris itu dipakai penuh oleh lirik bila ada baris lirik sinkron yang tampil. Jam di notch kecil saat idle tidak berubah.

## [0.11.0] - 2026-10-30

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #37, #38, #39, #40).

### Added
- **Ganti perangkat output audio dari slide Music** (PR #37): tombol ikon speaker atau headphone di kiri tombol "sebelumnya"; klik membuka daftar perangkat putar aktif (nama tebal, bagian driver lebih redup, perangkat sekarang diberi centang), dan klik satu baris mengganti perangkat default untuk ketiga peran (konsol, multimedia, komunikasi) lewat `IPolicyConfig::SetDefaultEndpoint`. Roda mouse pada tombol berpindah ke perangkat berikutnya atau sebelumnya. Bila pergantian gagal, aplikasi membuka `ms-settings:sound` dan selebihnya diam. Daftar dan ikon mengikuti Windows (headset dicolok atau dicabut). Tidak ada switcher mikrofon.
- **Progres unduhan di notch** (PR #38, Settings > Alerts > "Download progress", nyala secara default): unduhan browser (`.crdownload`, `.part`) di folder Downloads tampil sebagai "↓ 4,1 MB/s" di slot yang sama dengan hitung mundur sholat dan rapat, dengan garis tipis di dasar notch; slide Ledge mendaftar unduhan yang berjalan (nama, ukuran, kecepatan; klik membuka berkas di Explorer); dan toast "Downloaded" dengan tombol **Open** dan **Show in folder** saat selesai. Hanya ukuran dan kecepatan, tanpa persen, dan hanya satu folder (Downloads, mengikuti folder yang dipindah). Tanpa polling saat tidak ada unduhan, dan tidak ada yang berjalan saat pengaturan mati.
- **Waktu sholat lebih lengkap** (PR #40): **Imsak** (Subuh dikurangi 10 menit) dan **Terbit** (matahari terbit), keduanya dicocokkan dengan jadwal Kemenag; **slide Prayer** baru (tampil bila "Prayer times" nyala) yang mendaftar tujuh waktu hari ini, meredupkan yang lewat dan menyorot yang berikutnya beserta sisa waktunya; **mode Ramadan** manual (Settings > Widgets > "Ramadan mode"): hitung mundur "Imsak 12m" dan "Iftar 12m" serta toast Imsak dan Iftar; dan **suara adzan** opsional (mati secara default): Off, System sound, atau File (.mp3 atau .wav pilihan pengguna, diputar lewat MCI; aplikasi tidak menyertakan rekaman). Tidak berbunyi saat aplikasi layar penuh di depan, dan ada tombol **Stop** di toast dan slide Prayer.

### Changed
- **Mata idle lebih hemat CPU** (PR #39): saat diam, mata digambar sekitar 12 fps (sebelumnya 25 fps) dan berhenti total setelah 5 menit tanpa input keyboard atau mouse; kedip dan lirikan tetap 60 fps. Kecepatan glow tidak berubah.

## [0.10.2] - 2026-10-29

### Added
- **Pasang pembaruan dengan satu klik**: pada toast "Version X is available", tombol kini **Install** (bila rilisnya punya installer dan berkas `SHA256SUMS`). Aplikasi mengunduh `HesewakeNotch-Setup-X.exe` dengan progres di toast, memeriksa ukuran dan **SHA-256**-nya terhadap `SHA256SUMS` di rilis yang sama, menyimpannya di folder sementara, lalu menjalankan installer biasa (wizard-nya tetap terlihat; tidak ada instal senyap). Berkas hanya diunduh dari `github.com/dirgaastya/hesewake-notch/releases/download/`, ukurannya harus wajar, dan bila unduhan atau checksum gagal, toast menawarkan halaman rilis. Rilis lama tanpa `SHA256SUMS` tetap membuka halaman rilis seperti sebelumnya. Workflow rilis kini menerbitkan `SHA256SUMS` di setiap rilis. Karena installer belum ditandatangani, PC dengan Smart App Control atau kebijakan Application Control tetap bisa memblokirnya, sama seperti unduhan manual.

## [0.10.1] - 2026-10-29

### Fixed
- **Tombol Download pada toast pembaruan tidak bisa dipencet**: begitu pointer mendekati toast, notch ikut terbuka dan membuka slide, sehingga toast hilang sebelum tombolnya terklik. Hover di atas toast yang punya tombol (pengingat Water, **Join** pada rapat, **Download** pada pembaruan) kini tidak lagi membuka notch; sebelumnya hanya toast Water yang dikecualikan.

## [0.10.0] - 2026-10-29

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #32).

### Added
- **Mute mikrofon** dengan `Ctrl+Alt+M` untuk semua aplikasi (`IAudioEndpointVolume::SetMute` pada perangkat rekam default; bila perangkat untuk peran konsol dan komunikasi berbeda, keduanya di-mute). HUD singkat menampilkan **Muted**, **On**, atau **Not found** bila tidak ada mikrofon. Perubahan mute dari tempat lain (pengaturan Windows, tombol mic di keyboard, aplikasi lain) diikuti langsung tanpa polling. Pil privasi di sisi kanan notch kecil menampilkan mic tercoret selama di-mute, oranye bila ada aplikasi yang masih memakai mic dan abu-abu bila tidak, jadi status mute tidak pernah tersembunyi; toast "Microphone in use" ikut menampilkannya. Bisa dimatikan di Settings > Alerts > "Mic mute key" (kontribusi @MaulanaRhezaArdiansyah, PR #32).
- **Lirik cadangan dari [lrc.red](https://lrc.red)**: bila LRCLIB tidak punya lirik sinkron untuk sebuah lagu, aplikasi mencari di lrc.red (judul + artis, lalu judul saja). Hasil hanya dipakai bila judulnya sama persis (versi lain seperti "(Acoustic)" tidak ikut), artisnya cocok, dan durasinya selisih paling banyak 3 detik. Penanda waktu per kata (`<00:19.76>`) dibuang supaya baris tampil bersih. Permintaan hanya terjadi sekali per lagu, dan hanya bila LRCLIB tidak punya lirik sinkron. Sesuai syarat lrc.red, permintaan memakai User-Agent yang menyebut aplikasi, tautan, dan email kontak, dan sumber lirik dikreditkan: label kecil "via lrc.red" / "via LRCLIB" (klik membuka situsnya) di pojok panel mode lirik, serta di kotak About.
- Unison (Better Lyrics) ikut dicoba sebagai sumber lain, tetapi dilewati: tujuh pencarian lagu populer, Jepang, dan Indonesia semuanya tidak ditemukan karena kuncinya video YouTube.

- **Lirik Korea dalam romanisasi** (Settings > Widgets > "Romanize lyrics", menggantikan "Romaji lyrics"): suku kata Hangul dilatinkan dengan *Revised Romanization*, sistem resmi Korea Selatan, **tanpa kamus dan tanpa unduhan**. Aturan bunyi antar suku kata diterapkan dalam satu kata: liaison (한국어 = *hangugeo*), nasalisasi (국물 = *gungmul*), lateralisasi (신라 = *silla*), aspirasi dengan ㅎ (놓다 = *nota*), dan palatalisasi (같이 = *gachi*); sesuai RR, pengerasan bunyi tidak ditulis (학교 = *hakgyo*). Karena Korea tidak butuh kamus, toggle ini kini selalu bisa dinyalakan, dan unduhan kamus IPADIC (khusus Jepang) pindah ke baris tersendiri **"Japanese dictionary"** (Get, lalu Delete dengan dua kali tekan). Diuji dengan puluhan kata dan dengan lirik nyata.

### Changed
- **Settings dirapikan**: tab kini **Slides, General, Alerts, Widgets** (sebelumnya Slides, Display, Features, Alerts, Extras), dikelompokkan menurut tujuan. **General**: Language, Accent, Autostart, Slide hotkeys, Monitor, Notch size, Minimal notch, Thin on maximize, Idle display. **Alerts**: Notifications, Contact photos, Volume HUD, Brightness HUD, Lock keys, Mic / camera dot, Update notice. **Widgets**: Weather, City, Prayer times, Calendar, Lyric timing, Romaji lyrics (dan Delete kamus).
- **Baris bisa di-scroll**: tab General, Alerts, dan Widgets menampilkan tiga baris sel sekaligus dan sisanya digulir dengan roda mouse (scrollbar tipis di kanan), sehingga tidak ada lagi batas enam pengaturan per tab. Sel kini tersusun kiri ke kanan, atas ke bawah. Pindah tab selalu mulai dari atas.
- Di kode, klik pengaturan kini dikenali lewat id baris (`Rid`), bukan nomor urut, jadi menambah atau memindah pengaturan tidak lagi menggeser nomor yang lain.

## [0.9.0] - 2026-10-29

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #30).

### Added
- **Pemberitahuan pembaruan** (Settings > Alerts > "Update notice", default nyala): sekitar semenit setelah aplikasi mulai, lalu tiap 24 jam, aplikasi menanyakan rilis terbaru ke GitHub (`releases/latest`) di thread latar. Bila ada versi yang lebih baru, muncul toast "Version X is available" dengan tombol **Download** yang membuka halaman rilis, sekali per versi (versi yang sudah dilihat disimpan sebagai `update_seen` di `settings.txt`). Tidak ada unduhan atau instal otomatis. Bila setting dimatikan, tidak ada permintaan otomatis.
- **Menu klik kanan**: **Check for updates** selalu menjawab dengan toast (ada versi baru, "You're up to date", atau gagal), dan **Update available (X)...** muncul selama ada versi yang lebih baru.
- Hanya halaman di `github.com/dirgaastya/hesewake-notch/releases/` yang dibuka; draft, pre-release, dan tag non-versi diabaikan; versi dibandingkan secara numerik. Kegagalan (offline, rate limit) diam saat pengecekan otomatis (kontribusi @MaulanaRhezaArdiansyah, PR #30).

## [0.8.0] - 2026-10-29

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #20, #27).

### Added
- **Rapat berikutnya dari link ICS** (Settings > Features > "Calendar"): salin alamat rahasia kalender (Google: "Secret address in iCal format", Outlook: "Publish calendar"), lalu tekan **Paste**; link dibaca dari clipboard, diperiksa (https atau webcal), dan disimpan di **Windows Credential Manager** (`HesewakeNotch/calendar-link`), tidak pernah di file. Tekan **Remove** dua kali untuk melepas. Tanpa OAuth dan tanpa API key; kalender diambil di thread latar tiap 10 menit, satu kalender.
- **Hitung mundur rapat**: 10 menit terakhir sebelum rapat, notch menampilkan "Standup 8m" di tempat yang sama dengan hitung mundur sholat (idle dan di samping lagu); bila keduanya aktif, yang lebih dekat menang. Saat rapat dimulai muncul toast berisi judul dan jam, dengan tombol **Join** bila acara punya link Meet, Zoom, Teams, atau Webex.
- **Slide Agenda** (opsional, tersembunyi secara default): sisa acara hari ini (acara seharian di atas), rapat yang berjalan diberi "Now", tombol Join, dan bisa di-scroll. Bila kosong, slide menjelaskan sebabnya (belum ada link, memuat, gagal, atau "Nothing else today" beserta acara berikutnya).
- **Settings > Slides** kini memuat 10 slide: grid tiga per baris sesuai urutan carousel, bisa di-scroll dengan roda mouse dan scrollbar tipis; slide ke-10 mendapat `Ctrl+Alt+0`.
- Pembaca iCalendar `src/ics.rs`: UTC, `TZID` (dengan `VTIMEZONE` milik feed, termasuk DST), floating, dan seharian; `RRULE` DAILY/WEEKLY/MONTHLY/YEARLY dengan `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY`, `BYMONTHDAY`, `BYMONTH`; `EXDATE`, instance yang dipindah (`RECURRENCE-ID`) dan dibatalkan; `DURATION`. Feed dianggap tidak tepercaya: tidak panik pada input rusak, kerja dibatasi per feed dengan jatah adil per acara, unduhan di atas 10 MB ditolak, dan hanya host Meet/Zoom/Teams/Webex asli yang dianggap link Join (kontribusi @MaulanaRhezaArdiansyah, PR #20).

### Fixed
- **Garis tipis mode busy** tidak lagi menyisakan potongan sampul album dan equalizer setelah toast (misalnya HUD volume) menutup saat notch menyusut kembali (kontribusi @MaulanaRhezaArdiansyah, PR #27).

## [0.7.0] - 2026-10-29

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #19).

### Added
- **Bahasa UI Indonesia** (Settings > Features > "Language": English / Indonesian). Semua teks antarmuka berganti seketika tanpa restart: slide, Settings, toast, menu klik kanan dan tray, serta kotak About. Disimpan sebagai `lang=` di `settings.txt`; bawaannya tetap English, jadi pengguna lama tidak berubah. Nama produk dan fitur (Hesewake Notch, Ledge, Scrap, Pomodoro, Stopwatch, Timer, HUD), nama kota, dan nama waktu sholat tidak diterjemahkan. Semua teks Indonesia ada di satu tabel di `src/i18n.rs`; sebuah tes memindai setiap `tr("...")` dan gagal bila ada terjemahan yang hilang, tidak terpakai, ganda, atau placeholder-nya tidak cocok. Tes lebar teks Settings kini berjalan untuk kedua bahasa. Tab Settings disesuaikan lebarnya sehingga tab bahasa Inggris tidak lagi melewati tepi notch 2 px (kontribusi @MaulanaRhezaArdiansyah, PR #19).

## [0.6.0] - 2026-10-28

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #17, #18) dan Dependabot (PR #11).

### Added
- **Slide Clipboard** (opsional, tersembunyi secara default; nyalakan di Settings > Slides, hotkey `Ctrl+Alt+9`): menyimpan 10 teks terakhir yang disalin dari aplikasi mana pun, terbaru di atas. Klik baris untuk menyalin ulang (baris menampilkan "Copied"), tombol **×** menghapus satu entri, dan **Clear** mengosongkan daftar. Menyalin teks yang sama memindahkannya ke atas tanpa entri ganda. Hanya teks, hanya di memori (tidak pernah ditulis ke disk), dan clipboard tidak dibaca sama sekali saat slide mati. Salinan dari password manager (`ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`, `CanIncludeInClipboardHistory = 0`) dan salinan di atas 100.000 karakter dilewati (kontribusi @MaulanaRhezaArdiansyah, PR #18).
- **Tes snapshot** untuk Pomodoro, waktu sholat, warna aksen, dan hotkey slide (kontribusi @MaulanaRhezaArdiansyah, PR #17).

### Changed
- GitHub Actions `actions/checkout` naik dari 5.1.0 ke 7.0.1 (Dependabot, PR #11).

## [0.5.0] - 2026-10-27

Rilis ini berisi kontribusi dari @MaulanaRhezaArdiansyah (PR #2, #3, #4, #5, #6, #8, #9) dan @gewall (PR #7).

### Added
- **Waktu sholat** (Settings > Extras > "Prayer times", default nyala): hitung mundur 15 menit sebelum tiap waktu sholat dan toast saat waktunya masuk. Di notch idle, slot kiri jam bergantian suhu, ikon cuaca, lalu label seperti "Ashar 12m"; saat musik diputar, label ada di kanan judul/lirik. Dihitung lokal tanpa internet (rumus posisi matahari dengan parameter Kemenag RI, dicocokkan dengan jadwal resmi), mengikuti kota yang dipilih di Settings dan zona waktu Windows (kontribusi @MaulanaRhezaArdiansyah, PR #2).
- **Mode Pomodoro** di slide Timer (kontribusi @MaulanaRhezaArdiansyah, PR #4): siklus otomatis fokus 25 menit lalu istirahat 5 menit, dengan istirahat panjang 15 menit setiap 4 sesi. Toast di tiap pergantian fase, titik progres siklus, penghitung sesi selesai, dan tombol **Skip**. Pil di notch idle merah saat fokus dan hijau saat istirahat. Flag uji `--demo-timer=pomo` dan `--demo-toast=pomo`.
- **Warna aksen** (Settings > Extras > "Accent": Blue, Purple, Pink, Orange, Green, Teal) yang dipakai toggle, chip, judul slide, tombol, titik navigasi, progress musik, HUD, ring dan pil Timer, grafik System, dan cahaya bawah slide. Warna bermakna (peringatan merah, hijau "selesai", biru Water dan Bluetooth, glow Music dari sampul album) tidak berubah (kontribusi @MaulanaRhezaArdiansyah, PR #8).
- **Hotkey slide**: `Ctrl+Alt+1..8` membuka notch langsung di slide ke-N yang aktif (urutan mengikuti Settings > Slides). Menekan angka yang sama lagi menutup notch; angka tanpa slide diabaikan. Nomornya tampil sebagai keycap kecil di Settings > Slides, dan bisa dimatikan di Settings > Features > "Slide hotkeys" (di keyboard dengan AltGr, mis. Jerman, Ctrl+Alt+angka mengetik karakter) (kontribusi @MaulanaRhezaArdiansyah, PR #9).
- **Lirik Jepang dalam romaji** (Settings > Extras > "Romaji lyrics", default mati): kanji dan kana di lirik sinkron diubah ke romaji di notch kecil, slide Music, dan mode lirik. Bacaan per kata dari [Lindera](https://github.com/lindera/lindera) (kamus IPADIC), lalu katakana ke romaji lewat [WanaKana](https://docs.rs/wana_kana); partikel memakai pelafalan (は = wa, へ = e). Konversi dikerjakan sekali per lagu di thread lirik, hanya bila lirik berisi kana (lirik Mandarin tidak disentuh), dan kamus dilepas lagi setelahnya (kontribusi @gewall, PR #7).
- Kamus IPADIC (±47 MB, tidak ditanam di exe dan tidak ikut installer) diunduh lewat tombol **Get** di baris "Romaji lyrics" (unduhan 10 MB dengan persen kemajuan, tombol Retry bila gagal) ke `%APPDATA%\hesewake-notch\dict\ipadic`. Arsip dicek ukuran dan SHA-256-nya, diekstrak ke folder sementara lalu dipindah; setelah selesai romaji langsung aktif untuk lagu yang sedang main. Kamus hasil unduhan bisa dihapus lagi lewat baris "Romaji dictionary" (tombol **Delete**, tekan dua kali untuk konfirmasi); romaji ikut dimatikan.
- **Tab "Alerts"** di Settings: Volume HUD, Brightness HUD, Lock keys, dan Mic / camera dot dipindah ke sini supaya setiap tab muat di grid 2 x 3 setelah ada pengaturan baru.
- **CI dan rilis di GitHub** (kontribusi @MaulanaRhezaArdiansyah, PR #3, #5): workflow build + tes tiap push dan PR (peringatan kompilator dianggap error), workflow rilis yang membuat draft GitHub Release dari tag `v*`, dan tes snapshot UI (PNG tiap slide diunggah dari CI).

### Fixed
- **Label Settings tidak lagi menimpa toggle dan stepper**: lebar sel 178 jadi 198 px, lebar teks mengikuti kontrolnya dan dipotong dengan "..." bila terlalu panjang, serta dua sub-label dipendekkan (kontribusi @MaulanaRhezaArdiansyah, PR #6).
- **Pil timer tidak lagi hilang saat cuaca disembunyikan** (kontribusi @MaulanaRhezaArdiansyah, PR #4).
- Panik `Gfx::rect` di build debug (assert tiny-skia) dan tiga peringatan kompilator (kode mati) diperbaiki (kontribusi @MaulanaRhezaArdiansyah, PR #3, #5).
- **Bacaan romaji yang sering salah** dikoreksi setelah diuji dengan lirik asli: 二人 = "futari", 角 = "kado", 体中 = "karadajuu", 君 yang berdiri sendiri = "kimi", あなたがた = "anatagata", serta kata kerja yang kini menyatu ("kusunda", "waratteru", "wakachiau").

## [0.4.1] - 2026-10-27

### Added
- **Idle display** di Settings > Display: **Cycle** (bawaan: 10 menit jam, 15 menit mata, bergantian), **Clock** (jam saja), atau **Eyes** (mata saja). Pada Clock/Eyes, Ctrl+Alt+H dan tombol H tidak mengganti tampilan.

### Fixed
- **Notch idle tidak lagi "tertahan" di fase mata setelah ada aktivitas.** Hitungan siklus jam/mata dulu hanya dibekukan saat notch dipakai lalu dilanjutkan, jadi setelah notch dibuka, ada toast, atau musik berhenti, mata yang tertunda bisa muncul lagi (bisa sampai 15 menit). Kini setiap aktivitas mengembalikan siklus ke awal fase jam.
- Bila tombol pintas Ctrl+Alt+H gagal didaftarkan (sudah dipakai instance lain), kegagalannya dicatat di perf.log, tidak diam saja.

## [0.4.0] - 2026-10-26

### Added
- Skala DPI: notch mengikuti scaling Windows di monitor tempatnya berada, dengan pilihan "Notch size" di Settings (kontribusi dari @MaulanaRhezaArdiansyah, PR #1).
- Flag uji `--demo-notifs`: slide Notifications berisi data palsu untuk uji visual tanpa membaca notifikasi asli.
- **Emoji** di notifikasi, to-do, dan judul lagu tampil sebagai ikon monokrom (Segoe UI Emoji lewat GDI, indeks glyph dari tabel `cmap`), bukan kotak kosong. Penanda variasi, ZWJ, dan pengubah warna kulit tidak lagi menyisakan kotak. Emoji berwarna penuh belum didukung.
- **Pengingat minum berupa toast "Time to Drink"** dengan tombol **Snooze** (tunda 10 menit) dan **Drank** (catat satu Glass), menggantikan membuka slide Water selama 10 detik. Hover di toast tidak membuka notch; klik di luar tombol membuka slide Water. Bila notch sedang terbuka, pengingat tetap tampil di dalam slide seperti sebelumnya. Flag uji `--demo-toast=water`.
- **Settings Water baru** (tombol kembali bulat di header): stepper **Bottle** dan **Glass**, chip **Goal** dan **Remind**, **Hours** (jam mulai dan selesai pengingat, menggantikan batas 08:00-22:00 yang tetap), kartu statistik **reminders** (pengingat yang sudah muncul hari ini) dan **refills** (berapa kali botol habis), serta tombol **Reset today** yang menghapus progres, pengingat, dan refill hari itu. `water.txt` mendapat field opsional (bottle, jam mulai/selesai, jumlah pengingat); berkas lama tetap terbaca. Flag uji `--demo-water-settings`.
- **Slide Timer** (default tersembunyi; aktifkan di Settings > Slides): **stopwatch** dengan persepuluh detik dan ring menit, serta **hitung mundur** dengan preset 1/5/10/15/25/45 menit dan penyesuaian ±1 menit. Saat berjalan dan notch idle, waktunya tampil di kanan jam dalam pil biru berisi ring progres (notch melebar mengikuti isi dengan margin dan jarak yang sama antara cuaca, jam, pil, dan lonceng; di fase mata tetap berbeda jelas dari mata); hitung mundur yang habis memunculkan toast "Time's up". Mode dan durasi terakhir diingat di `timer.txt`. Flag uji `--demo-timer=count|stop`, `--demo-toast=timer`, `--demo-eye`.
- Flag uji `--demo-playing` (lagu palsu) dan `--demo-priv=cam,mic` (titik kamera/mikrofon) untuk uji tata letak notch kecil.

### Fixed
- **Titik indikator mikrofon/kamera menimpa lonceng notifikasi** (saat idle) dan lonceng, equalizer, serta teks judul/lirik (saat lagu diputar). Titik kini punya slot sendiri di tepi kanan, sejajar tengah; notch melebar sebesar slot itu dan elemen lain bergeser ke kiri.

### Changed
- **Teks lebih mudah dibaca** di semua slide dan toast: warna teks sekunder (isi pesan, label, satuan) dibuat lebih terang lewat satu konstanta bersama (`TEXT_DIM` / `TEXT_FAINT`), teks isi yang sebelumnya 10-11 px dinaikkan ke 11-12 px (isi dan waktu notifikasi, label System, sumbu grafik Water, keterangan Settings), dan tepi huruf terang di atas latar gelap diperkuat sedikit agar goresan tipis tidak hilang.

## [0.3.2] - 2026-10-25

### Fixed
- **Lag saat notifikasi muncul**, terutama pesan panjang atau yang berisi emoji, tanda kutip melengkung, dan simbol. Dua penyebab:
  - Pemotongan teks dengan "…" memangkas satu karakter lalu mengukur ulang seluruh string, jadi pesan panjang jadi kuadratik dan dihitung ulang tiap frame untuk tiap baris. Kini satu putaran dengan hasil identik (400 karakter: 7,4 ms jadi 8 µs). Waktu gambar toast turun dari ±3,7 ms ke ±0,28 ms per frame.
  - Karakter yang tidak ada di font bawaan memicu pemuatan font Segoe UI penuh ke `fontdue`: frame macet ±95-160 ms dan memori naik dari ±12 MB ke ±50 MB (lonjakan memori sporadis yang dulu belum terjelaskan). Kini karakter itu dirender per karakter lewat GDI (jalur yang sama dengan teks Jepang), tanpa memuat font besar: frame terlama ±6 ms dan memori tetap ±12 MB.

## [0.3.1] - 2026-10-25

### Fixed
- **Teks Jepang, Mandarin, dan Korea** (judul lagu, lirik, notifikasi, to-do) sebelumnya tampil sebagai kotak kosong karena font bawaan tidak punya kana, kanji, atau hangul. Karakter CJK kini dirender lewat GDI dari font sistem (Yu Gothic UI, Meiryo UI, MS Gothic, Microsoft YaHei UI, Malgun Gothic) dan masuk ke cache glyph yang sama. Memuat font CJK ke `fontdue` sempat dicoba tetapi menaikkan memori sekitar 420 MB, jadi tidak dipakai; jalur GDI tidak menambah memori.
- Lirik tidak lagi dicari (dan "Loading lyrics" tidak muncul) untuk media yang bukan lagu: sumber/judul/artis berisi "Netflix", atau durasi 20 menit ke atas (film, episode, podcast). Sebelumnya judul serial memunculkan lirik lagu yang salah.
- Flag uji `--demo-text=judul|isi` untuk melihat notifikasi dengan teks non-Latin.

## [0.3.0] - 2026-10-25

### Added
- **HUD volume dan kecerahan** di notch kecil: bar dengan ikon speaker/matahari dan persen, muncul saat volume berubah dari luar (tombol media, mixer Windows) atau kecerahan layar berubah (tombol Fn, slider Windows). Volume dipantau lewat callback `IAudioEndpointVolume` (otomatis pindah saat perangkat audio default berganti), kecerahan lewat notifikasi daya `GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS` (terbukti di laptop ini). Perubahan beruntun memperbarui bar di tempat. Toggle di Settings > Features. HUD bawaan Windows tetap muncul (tidak bisa dimatikan secara resmi).

- **Slide System** (default **tersembunyi**; nyalakan di Settings > Slides): CPU dengan sparkline, memori (GB + ring), jaringan (unduh/unggah + sparkline, hanya adapter fisik yang aktif), dan baterai (persen, status, sisa waktu). Pengukuran hanya berjalan selama slide terlihat.
- Tab Slides di Settings kini 3 kolom agar muat lebih banyak slide; slide opsional baru dari versi mendatang otomatis tersembunyi.
- **Cuaca di kiri jam** (notch idle, tanpa lagu): suhu ("27°") dan ikon kondisi bergantian tiap ±4 dtk dengan transisi geser + fade. Ikon vektor beranimasi dari kode cuaca WMO + siang/malam: matahari, bulan, berawan, hujan/gerimis (tetesan jatuh), salju, kabut, petir. Varian **panas**: cerah/berawan tipis siang hari dengan suhu >= 32 C memakai matahari oranye berdenyut. Data dari Open-Meteo (tanpa API key) tiap 30 menit di thread latar belakang, di-cache di `weather.txt`; jaringan hanya dipakai saat fitur aktif. Toggle dan pilihan kota (36 kota Indonesia, termasuk Karawang) di Settings > Display.
- Flag uji `--demo-weather=kode,suhu,siang`.
- **Multi-monitor**: pilih monitor notch di Settings > Display > Monitor: Primary, Follow (pindah ke monitor tempat kursor berada, hanya saat notch tidak dipakai), atau Display 1, 2, ... Notch dipindah ulang otomatis saat monitor dicolok/dicabut (`WM_DISPLAYCHANGE`); monitor yang hilang jatuh kembali ke layar utama. Deteksi layar penuh dan zona hover kini memakai monitor notch, bukan layar utama saja.
- Settings punya tab keempat **Extras** (HUD volume/kecerahan, cuaca, kota); Notifications dan Contact photos dikelompokkan di Features.

### Fixed
- Pesan daya Windows (kecerahan, charger, layar mati/hidup) kini diteruskan lewat pesan yang di-post. Sebelumnya bisa terlewat bila tiba saat state aplikasi sedang dipegang, sehingga toast charger atau pemulihan timer setelah layar hidup berisiko hilang.
- HUD (tombol kunci, volume, kecerahan) tidak lagi diantre saat notch sedang terbuka atau toast lain tampil, sehingga tidak muncul terlambat.

## [0.2.0] - 2026-10-24

### Added
- **Slide Settings** dengan tiga tab. Slide ini **hanya muncul di carousel saat dibuka** lewat klik kanan/tray > "Settings", lalu hilang lagi setelah kamu pindah slide atau notch menutup (tidak memakan tempat di titik slide):
  - **Slides**: tampilkan/sembunyikan tiap slide dan ubah urutannya dengan tombol naik/turun (sel bertukar tempat dengan animasi).
  - **Display**: Minimal notch, "Thin on maximize", penyesuaian waktu lirik (-500 sampai +1000 ms), foto kontak pada notifikasi.
  - **Features**: notifikasi Windows dan autostart.
  Menu klik kanan dan tray punya item "Settings".
- **Foto profil kontak pada notifikasi** (mis. WhatsApp Web): avatar bulat di toast dan daftar, dengan logo aplikasi sebagai lencana kecil. Gambar diambil dari payload toast di `wpndatabase.db` (dibuka read-only, hanya satu id, hanya berkas lokal) dan dari salinan yang dibuat pemantau folder `Notification Resources` milik Chrome/Edge/Brave, karena browser menghapus berkas gambarnya sesaat setelah toast tampil. Bisa dimatikan di Settings > Display > "Contact photos".
- **Toast tombol kunci**: Caps Lock, Num Lock, dan Scroll Lock menampilkan HUD kecil On/Off di notch (hook keyboard low-level yang hanya terpasang saat fitur aktif; toggle di Settings > Features > "Lock keys"). HUD beruntun menggantikan yang sedang tampil tanpa antre.
- **Indikator mikrofon/kamera**: titik oranye (mic) atau hijau (kamera) di pojok kanan atas notch kecil selama ada aplikasi yang memakainya, plus toast "Microphone/Camera in use" dengan nama aplikasi saat mulai. Dipantau lewat event registry `CapabilityAccessManager` (tanpa polling). Toggle di Settings > Features > "Mic / camera dot".
- **Musik**: scroll mouse di atas baris kontrol slide Music mengatur **volume master** (2% per takik) dengan indikator di dalam slide; jika ada lebih dari satu pemutar media, header "Now Playing · <pemutar>" bisa diklik untuk berpindah sumber.
- **Installer** (`hesewake-notch.iss`, Inno Setup 6) dan `scripts\build-installer.ps1`: pasang per-pengguna tanpa admin, pilihan autostart, menutup aplikasi yang sedang jalan sebelum memperbarui, dan opsi menghapus data saat uninstall.
- Registri slide (`SlideId`) dan model pengaturan (`settings.txt`: `slides=`, `lyric_lead_ms=`, `busy_on_maximized=`, ...) yang kompatibel dengan berkas lama.

### Changed
- Menu klik kanan disederhanakan: item air (Remove 250 ml, Snooze, Reset, goal harian, interval pengingat) dihapus; semuanya ada di slide Water (tombol Settings dan Snooze).
- **Runtime C/C++ ditanam ke exe** (static CRT): tidak lagi butuh Microsoft Visual C++ Redistributable di PC lain (exe ±1,9 MB).
- Glow mata saat idle memakai palet **biru** (sebelumnya merah), serasi dengan tema.
- Jendela **maximized biasa tidak lagi** membuat notch menyusut jadi garis tipis; hanya layar penuh sungguhan dan aplikasi Adobe. Bisa dikembalikan lewat Settings > Display > "Thin on maximize".
- Pindah slide, titik slide, dan hover lonceng memakai identitas slide (bukan nomor tetap), jadi tetap benar saat slide disembunyikan atau diurutkan ulang.

### Fixed
- Menempatkan kursor dengan klik mouse di kartu Scrap tidak berfungsi karena memeriksa nomor slide yang salah.

## [0.1.0] - 2026-10-24

Rilis pertama **Hesewake Notch**, fork pribadi dari [Poch](https://github.com/klawcodes/poch) 0.7.0 oleh Commonly Works (MIT).

### Added
- **Lirik sinkron** dari LRCLIB: baris aktif tampil di notch kecil (notch melebar mengikuti panjang baris, teks tebal, tanpa running text) dan di slide Music.
- **Mode lirik** di slide Music (tombol balon): panel kiri cover, judul, progres, kontrol; panel kanan daftar lirik yang meluncur halus mengikuti baris aktif. Klik baris untuk lompat ke posisi lagu itu.
- **Cover album** menggantikan ikon aplikasi di notch kecil saat lagu main, dengan transisi pegas.
- **Water baru**: angka besar menghitung naik, toggle pengingat, progress bar gradasi, tombol Drink dan Settings, grafik batang per jam, pilihan goal/interval/ukuran gelas di dalam slide, tombol Snooze saat pengingat aktif.
- **Toast di notch**: charger tersambung, charger dicabut, baterai rendah 20%, dan perangkat audio Bluetooth tersambung/terputus, dengan animasi pegas dan riak drop-wave.
- **Notifikasi Windows di notch** (UserNotificationListener): toast saat notifikasi masuk, ikon lonceng dengan badge belum dibaca di notch kecil, dan slide **Notifications** (daftar, hapus per item, Clear all, scroll). Bisa dimatikan dari menu; aplikasi bisa dibisukan lewat `notif_mute.txt`.
- **Tray icon + menu**, **Start with Windows**, mode **Minimal notch**, indikator koneksi internet di slide Music, **single instance**, dan jeda render/polling saat layar mati.
- Ikon aplikasi baru (notch + mata, gradasi biru) dan tema aksen biru: glow slide To-do/Ledge/Scrap/Notifications, titik slide aktif, spektrum equalizer, dan progress bar lagu.
- Alat diagnosa: log performa/memori lewat `HESEWAKE_PERF=1` (`perf.log`), argumen `--demo-toast=...` dan `--demo-slide=N` untuk pratinjau visual.

### Changed
- **Rebrand** dari Poch ke Hesewake Notch: nama paket/exe, folder data (`%APPDATA%\hesewake-notch`), mutex, class window, dan entri autostart. Data dan autostart lama dimigrasi otomatis. About menyebut Poch sebagai dasar.
- **Performa**: `opt-level = 3`, animasi memakai waitable timer resolusi tinggi dan loop pesan sendiri (menggantikan `WM_TIMER`), cache glyph lebih besar dengan pemanasan awal, alokasi per frame dikurangi.
- Memori dipangkas ±tiap 60 detik (sebelumnya tidak pernah saat musik main) dan decode gambar dibatasi 32 MB.

### Fixed
- Cover lagu sebelumnya yang "nyangkut" (thumbnail GSMTC basi) dan cover yang hilang karena balapan antar-thread.
- Pencarian cover online gagal untuk judul berakhiran " / ", " - ...", atau "(feat. ...)"; ditambah percobaan ulang.
- Posisi lagu/lirik yang macet pada pemutar yang jarang memperbarui `LastUpdatedTime` (mis. Spotify).
- Tes tidak lagi menimpa `water.txt` asli.

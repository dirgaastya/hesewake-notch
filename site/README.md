# Project site

The static page published to GitHub Pages by `.github/workflows/pages.yml` whenever `site/` changes on `main`.

| File | What it is |
| --- | --- |
| `index.html` | The page. One file with its own CSS; no frameworks and no third-party requests. |
| `demo.html` | The interactive interface preview shown in the hero. A standalone copy of the project's design prototype, kept here as the source: edit it directly. All sample data (songs, people, tasks, events) is fictional. |
| `fonts/` | Instrument Sans (the app's own font, SIL Open Font License). |
| `img/slides/` | Pictures of the real slides, drawn by the app's own renderer with made-up sample data (see below). |
| `icon.ico` | The app icon. |

Preview locally with any static server, for example `python -m http.server --directory site`.

## Regenerating the slide pictures

The pictures come from the test `snap_site_slides` (`src/snapshot.rs`), so they show the real interface and stay honest
when it changes. It writes transparent, double-size PNGs:

```
SNAPSHOT_DIR=out SNAPSHOT_TRANSPARENT=1 cargo test --release snap_site_slides
```

Copy `out/site-<name>@200.png` to `site/img/slides/<name>.png` (music, scrap, agenda, prayer, timer, notifs). The sample
data (a made-up song, notes and notifications) is set in that test; no network request is made.

Rules for edits: keep the text accurate (the privacy table must match what the app really connects to), keep white-on-blue buttons at 4.5:1 contrast or better, and keep the page free of horizontal scroll at 320 px wide.

# Hesewake Notch extensions (protocol draft, version 1)

> **Status: draft for review. Nothing here is implemented yet.** It describes what the notch will offer to extensions
> from the first release that supports them, and what an extension has to do.

An extension is a small program that runs next to the notch and talks to it over a local **named pipe**. The notch draws
everything (slide, toasts, markers, animations); the extension only decides **what** to show and reacts to clicks. This
keeps extensions out of the notch's process (a crash or a hang in an extension cannot take the notch down) and keeps the
look consistent.

Examples in this document use an imaginary issue tracker. Nothing in the protocol is specific to one service.

## 1. What an extension can do (version 1)

| Capability | What it is |
|---|---|
| **Slide** (optional) | One slide in the carousel, with the extension's name and icon. It can show a *list page* or a *detail page* (see section 5). The user can hide it, move it or give it a hotkey like any other slide (`Ctrl+Alt+N`). |
| **Toast** | A short message in the small notch ("title", "subtitle", optional count). Clicking it opens the extension's slide. |
| **Marker** | A dot on the slide's navigation dot, until the slide is opened ("something new"). |
| **Open a link** | Ask the notch to open an `http`/`https` link in the browser, after a click on the slide. |
| **Copy text** | Ask the notch to put a short text on the clipboard, after a click on the slide. |
| **Open its own settings** | The slide can show a button that asks the extension to open its own settings window. |

Out of scope for version 1: more than one slide per extension, buttons inside toasts, images, custom drawing, global
hotkeys owned by an extension, and anything that runs inside the notch's process.

## 2. Installing and starting

An extension is registered by a **manifest** in the user's data folder:

```
%APPDATA%\hesewake-notch\extensions\<id>\extension.json
```

```json
{
  "id": "example-issues",
  "name": "Issues",
  "version": "1.0.0",
  "exe": "C:\\Users\\me\\AppData\\Local\\Programs\\example-issues\\example-issues.exe",
  "args": ["--notch"],
  "slide": true,
  "proto": 1
}
```

- `id`: 1 to 32 characters, `a-z`, `0-9`, `-`. It must equal the folder name.
- `slide`: `true` (default) gives the extension a slide. With `false` it has no slide: it runs in the background (a
  service, a bridge to something else) and can still send `toast`, `status` and take part in *Settings > Extensions*.
- `exe`: absolute path. The notch starts it without a shell, with `args`, no window, when the notch starts and the
  extension is enabled. If it exits, the notch restarts it with a growing delay (5 s, 15 s, 60 s, then every 5 minutes).
- Extensions are **off until the user turns them on** in *Settings > Extensions* (the page lists each manifest, its
  status and an on/off switch). Installing a manifest alone never runs anything.
- The extension may also be started by something else (for example by the user). The notch accepts a connection from a
  program only if it is the `exe` of an enabled manifest (section 3).

### Managing extensions (Settings > Extensions)

Every manifest is listed with its name, version and a status line, and the user can:

| Control | What it does |
|---|---|
| On / off | Starts or stops the extension (`quit`, then the process is ended after 3 seconds if it is still there). Saved. |
| Toasts | Mutes the extension's toasts without stopping it. |
| **Settings** | Sends `config` so the extension opens its own settings window. Greyed out while it is not connected. |
| **Restart** | Stops and starts it again (for an extension that is stuck). |
| **Open folder** | Opens the manifest's folder in Explorer. |

The status line is the extension's last `status` text; otherwise the notch's own: *Off*, *Starting*, *Connected*,
*Reconnecting in 60 s*, *Could not start* (with the reason: file missing, access denied), *Rejected: protocol* or *Stopped
(crashed 3 times)*. Installing and removing an extension is the job of its own installer (it writes or deletes the
manifest); the notch never deletes files of an extension.

## 3. Connection and security

- Pipe name: `\\.\pipe\hesewake-notch-ext-<user SID>`.
- The pipe is created by the notch with a **DACL that allows only the current user**,
  `PIPE_REJECT_REMOTE_CLIENTS`, and `FILE_FLAG_FIRST_PIPE_INSTANCE`. Other users and the network cannot connect.
- After connecting, the notch reads the client's process id (`GetNamedPipeClientProcessId`), looks up its image path
  (`QueryFullProcessImageNameW`) and requires it to equal the `exe` of an enabled manifest whose `id` matches the `hello`
  message. Anything else is disconnected at once.
- The extension should check that the server is the notch the same way (`GetNamedPipeServerProcessId`, image path
  `hesewake-notch.exe`) before sending anything private.
- **Everything the extension sends is untrusted data.** The notch never interprets it as markup or code: text only, all
  lengths and counts are clipped to the limits in section 6, control characters are removed, and a message that does
  not parse is ignored (three bad messages in a row close the connection).
- This protects against other users and accidental misuse. It does **not** protect against a malicious program that
  already runs as the same user and can start or replace the extension's exe; that is outside what a local pipe can do.

## 4. Messages

Frames are `u32` little-endian length, then that many bytes of **UTF-8 JSON** (one object). Maximum frame: **64 KiB**.
Every object has `"t"` (the message type). Unknown types and unknown fields are ignored, so new things can be added
without breaking older extensions. A change that is not backwards compatible raises `proto`.

### 4.1 Handshake

Extension → notch, first message:

```json
{ "t": "hello", "proto": 1, "id": "example-issues", "name": "Issues", "version": "1.0.0" }
```

Notch → extension:

```json
{ "t": "welcome", "proto": 1, "app": "0.13.0", "lang": "en", "accent": [90, 170, 255], "limits": { "rows": 100, "text": 120 } }
```

or `{ "t": "reject", "reason": "disabled" }` (then the notch closes the pipe). `reason` is one of `disabled`,
`unknown`, `proto` (no common version), `busy` (already connected).

If the notch supports a newer `proto` than the extension, it answers with the extension's version. The other way round is
`reject`.

### 4.2 Extension → notch

**`slide`**: the whole content of the slide. Send it again, complete, whenever something changes (the notch replaces
what it had; there is no patching). `rev` is a number that grows, counted across `slide` and `detail` of one connection; the notch drops a message whose `rev` is not newer than the last one it took.

```json
{
  "t": "slide", "rev": 12,
  "title": "Is|sues",
  "state": "ready",
  "tabs": [ { "id": "all", "label": "All", "count": 7 }, { "id": "todo", "label": "To do", "count": 3 } ],
  "tab": "all",
  "actions": [ { "id": "refresh", "label": "Refresh" }, { "id": "report", "label": "Report" } ],
  "rows": [
    { "id": "128", "text": "#128 Fix crash on startup", "sub": "To do", "mark": true },
    { "id": "127", "text": "#127 Update the installer", "sub": "In progress" }
  ],
  "foot": "Updated just now"
}
```

- `title`: `|` splits the two-colour title, as in the notch's own slides. Optional.
- `state`: `ready`, `loading`, `empty` (shows `message`), `error` (shows `message` and a **Retry** button that sends
  `action` with `id: "retry"`), or `setup` (shows `message` and a button labelled `setup_label` that sends
  `action` with `id: "setup"`). Default `ready`.
- `tabs`: at most 8 filter chips; `tab` selects one. Clicking a chip makes the notch send `tab` (below); the extension
  answers with a new `slide`.
- `actions`: at most 3 buttons in the header. Like the buttons of a detail page, an action may carry `link` or `copy`
  (below); then the notch does it on the click and shows a short "Copied" mark for `copy`.
- `rows`: at most 100. `mark` draws the "new" dot. The notch scrolls the list itself.
- `sections`: instead of `rows`, at most 4 groups `{ "title": "Worked on", "rows": [...] }` (100 rows in all), each with
  its own heading. Use it for a page that has more than one list, such as a daily summary.
- `stepper`: `{ "label": "2026-10-31", "prev": true, "next": false }` draws previous / next arrows around a label in the
  header (for a date or a period). `prev` / `next` say whether each arrow is enabled. A click sends `step`.
- `back`: `true` draws a **Back** button in the header; a click sends `back`. The extension then sends the page to go
  back to. Without it the slide is the home page and has no Back button.
- `foot`: one short line under the list (optional).

**`detail`**: replaces the list with a detail page (with a **Back** button the notch draws). Send `slide` again to leave
it, or let the user click Back (the notch sends `back`).

```json
{
  "t": "detail", "rev": 13, "for": "128",
  "state": "ready",
  "title": "#128 Fix crash on startup",
  "meta": [ { "k": "Project", "v": "Installer" }, { "k": "Stage", "v": "To do" }, { "k": "Due", "v": "2026-11-05" } ],
  "body": "The app closes when the settings file is empty.\nSteps: ...",
  "notes": [ { "who": "Dewi", "when": "2026-10-31 14:02", "text": "Reproduced on 0.12.0." } ],
  "buttons": [ { "id": "open", "label": "Open", "link": "https://tracker.example/issues/128" }, { "id": "copy", "label": "Copy", "copy": "#128 Fix crash on startup" } ],
  "foot": "2 attachments"
}
```

- `meta`: at most 6 pairs. `body`: plain text, newlines kept, at most 4,000 characters. `notes`: at most 5, oldest first.
- `buttons`: at most 4. A button with `link` opens that `http`/`https` URL in the browser; with `copy` it puts that text
  on the clipboard (at most 8,000 characters). Both happen **only as the direct result of the user's click on that button**,
  so the extension cannot open pages or write the clipboard on its own. A button with neither only sends `action`.
- `state` works like in `slide` (`loading`, `error`, ...).

**`toast`**: show a message in the small notch (not while a full-screen app is in front, and only if the user has not
muted the extension).

```json
{ "t": "toast", "title": "#128 Fix crash on startup", "sub": "New · To do", "count": 1 }
```

**`badge`**: `{ "t": "badge", "on": true }` puts the "something new" dot on the slide's navigation dot; the notch clears
it when the user opens the slide (and sends `visible`).

**`status`**: `{ "t": "status", "text": "Phone connected" }` (at most 60 characters) is shown next to the extension in
*Settings > Extensions*. Send it whenever the state worth knowing changes ("Not signed in", "Syncing", "3 devices").
Extensions without a slide use it to tell the user what they are doing.

**`bye`**: `{ "t": "bye" }` before the extension exits on purpose, so the notch does not try to restart it.

### 4.3 Notch → extension

| Message | When |
|---|---|
| `{"t":"visible","on":true}` | The slide came into view (`on: true`) or left it. Refresh data when it opens; stop polling hard when nobody looks, if you like. |
| `{"t":"tab","id":"todo"}` | The user picked a filter chip. |
| `{"t":"row","id":"128"}` | The user clicked a row. Usually answered with `detail`. |
| `{"t":"action","id":"refresh"}` | A header button, **Retry**, **setup**, or a detail button without `link`/`copy`. |
| `{"t":"step","dir":-1}` | The user clicked an arrow of the `stepper` (`-1` previous, `1` next). |
| `{"t":"back"}` | The user clicked Back (a detail page, or a `slide` with `back: true`). |
| `{"t":"config"}` | The user pressed **Settings** for this extension in *Settings > Extensions*. |
| `{"t":"lang","lang":"id"}` | The notch language changed. |
| `{"t":"quit"}` | The notch is closing or the extension was turned off. Exit within 3 seconds. |

The notch also closes the pipe when it exits; the extension should then exit too.

## 5. What the slide looks like

The notch draws the pages with its own fonts and colours (the accent the user chose), light-weight animations and the same
scrolling as its other slides. The extension cannot change fonts, colours or layout. This is on purpose: it keeps the notch
readable and keeps untrusted data away from the drawing code. Texts longer than the space are cut with an ellipsis; the
notch tests all its own pages in English and Indonesian, so keep labels short (tab labels: 12 characters, row text: 120,
row sub: 40, button labels: 16).

## 6. Limits

| What | Limit |
|---|---|
| Frame | 64 KiB |
| Messages per second | 10 (the rest are dropped) |
| Tabs / header actions / rows (all sections together) | 8 / 3 / 100 |
| Detail meta / notes / buttons | 6 / 5 / 4 |
| Text lengths | ids 64, titles, meta keys, authors, dates, section and stepper labels 24, messages, detail titles and footers 120, tab label 12, action and button label 16, row text 120, row sub 40, meta value 80, note text 400, body 4,000, toast title 80, toast sub 60 |
| Copy text | 8,000 |
| Sections | 4 |
| Links | `http`/`https`, at most 500 characters, no credentials in the URL |
| Toasts | one every 5 seconds per extension; extra ones are dropped |

Characters are counted as Unicode scalar values. Anything over a limit is clipped, never rejected.

## 7. A minimal extension

```text
1. Write extension.json (section 2) into %APPDATA%\hesewake-notch\extensions\<id>\ .
2. Turn the extension on in Settings > Extensions. The notch starts the exe.
3. Connect to \\.\pipe\hesewake-notch-ext-<SID>, send "hello", wait for "welcome".
4. Send a "slide". React to "row", "tab", "action" and "visible".
5. Send "bye" and exit when you get "quit".
```

A small example extension will live in `examples/` once the protocol is implemented.

# Hesewake Notch extensions (protocol draft, version 1)

> **Status: version 1, implemented in the host (`ext.rs`, `extproto.rs`, `extapp.rs`, `extslide.rs`) and tried with the
> example in `examples/hello-extension`. It is not frozen yet: it will be when the first real extensions work end to end.**

An extension is a small program that runs next to the notch and talks to it over a local **named pipe**. The notch draws
everything (slide, toasts, markers, animations); the extension only decides **what** to show and reacts to clicks. This
keeps extensions out of the notch's process (a crash or a hang in an extension cannot take the notch down) and keeps the
look consistent.

Examples in this document use an imaginary issue tracker. Nothing in the protocol is specific to one service.

## 1. What an extension can do (version 1)

| Capability | What it is |
|---|---|
| **Slide** (optional) | One slide in the carousel, with the extension's name and icon. It can show a *list page* or a *detail page* (see section 5). It comes after the notch's own slides, in the order of the extensions list, and gets a `Ctrl+Alt+N` hotkey like the others. Switching the extension off in *Settings > Extensions* takes the slide away. |
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
  "settings": "window",
  "processes": "none",
  "proto": 1
}
```

- `id`: 1 to 32 characters, `a-z`, `0-9`, `-`. It must equal the folder name.
- `slide`: `true` (default) gives the extension a slide. With `false` it has no slide: it runs in the background (a
  service, a bridge to something else) and can still send `toast`, `status` and take part in *Settings > Extensions*.
- `processes`: `"none"` (default) or `"allow"`. By default the program cannot start other programs (see *Security*).
- `settings`: where the extension's settings are, `"window"` (default) or `"panel"`. See *Managing extensions* below.
- `exe`: absolute path. The notch starts it without a shell, with `args`, no window, when the notch starts and the
  extension is enabled. If it exits, the notch restarts it with a growing delay (5 s, 15 s, 60 s, then every 5 minutes).
- Extensions are **off until the user turns them on** in *Settings > Extensions* (the page lists each manifest, its
  status and an on/off switch). Installing a manifest alone never runs anything.
- The extension may also be started by something else (for example by the user). The notch accepts a connection from a
  program only if it is the `exe` of an enabled manifest (section 3).
- When the notch starts the extension, the full name of the pipe is in its environment as `HESEWAKE_NOTCH_PIPE`. A
  program started by anything else builds the name from the user's SID (section 3).

### Managing extensions (Settings > Extensions)

Every manifest is listed with its name, version and a status line, and the user can:

| Control | What it does |
|---|---|
| On / off | Starts or stops the extension (`quit`, then the process is ended after 3 seconds if it is still there). Saved. |
| Toasts | Mutes the extension's toasts without stopping it. |
| **Settings** | With `"settings": "window"` it sends `config`, and the extension opens its own settings window. With `"settings": "panel"` the extension's pages open **inside the Settings window**, in place of the list: the same `slide` and `detail` pages as for the notch (including `actions`, `tabs`, `stepper`, buttons and `matrix`), drawn in the style of the Settings window, and clicks come back as the same messages. The extension is sent `visible` with `on: true` when the panel opens and `on: false` when it closes; **Back** goes to the list. Greyed out while the extension is not connected. An extension with no slide uses the panel for everything it has to show. |
| **Restart** | Stops and starts it again (for an extension that is stuck). |
| **Open folder** | Opens the manifest's folder in Explorer. |
| **Remove** | Takes the extension out of the notch: it is stopped, switched off, and its manifest folder is deleted. Two presses (the first asks *Sure?*). Only a plain folder directly inside the extensions folder is deleted, never a link to somewhere else. A program installed somewhere else stays, for its own uninstaller. Also offered for a manifest that does not load. |

The status line is the extension's last `status` text; otherwise the notch's own: *Off*, *Starting*, *Connected*,
*Reconnecting in 60 s*, *Could not start* (with the reason: file missing, access denied), *Rejected: protocol* or *Stopped
(crashed 3 times)*. Installing an extension is the job of its own installer (it writes the manifest); taking it out of the
notch is **Remove**. See *Security* below for what an extension can and cannot do.

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
- `rows`: at most 100. `mark` draws the "new" dot. `tone` (`good`, `warn` or `muted`) colours the row's `sub` text and puts a dot before it (green, amber, grey). The notch scrolls the list itself.
- `hero`: a card at the top of the page, in the Settings panel: `{ "title": "Song", "sub": "Artist", "text": "Phone 1 - Playing", "image": { "mime": "image/jpeg", "data": "<standard base64>" } }`. The picture is PNG or JPEG of at most 48 KiB (decoded), drawn as a square from its middle; keep it small (about 150 px) so the whole page stays under the 64 KiB frame. Without a usable picture the card shows a plain tile. The carousel's slide does not draw a hero.
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
- `input`: a line of text the user can edit (the Settings panel): `{ "id": "rename", "label": "Name", "value": "Phone 1", "max": 32, "button": "Save" }`. `max` is 1 to 40. The user types in the box and presses Enter or the button, and the extension gets `{"t":"input","id":"rename","value":"..."}` (one line, control characters removed, at most 40 characters). Esc puts the old text back. The extension answers with a new page; until then the box keeps what was typed.
- `matrix`: a small black-and-white picture, such as a QR code: `{ "size": 3, "rows": ["101", "010", "111"] }`. `size` is 1 to 100, there are `size` rows, and each row is `size` characters of `0` (light) or `1` (dark). The notch draws it as squares on white, with a margin around it, on the right of the page. It is shown in the Settings panel, not on the carousel's slide. A matrix that is not exactly that is left out and the rest of the page stays. The notch never reads the picture, so an extension that shows a QR code encodes it itself.

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
| `{"t":"config"}` | The user pressed **Settings** for an extension with `"settings": "window"` in *Settings > Extensions*. |
| `{"t":"input","id":"rename","value":"Pixel 8"}` | The user confirmed the text of a page's `input`. |
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
| Matrix | 100 x 100 |
| Hero picture | 48 KiB decoded; hero lines 80 characters |
| Text box | 40 characters |
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

A complete small example is in `examples/hello-extension`. It uses only the standard library and `serde_json`, and shows
every kind of page, a toast, a badge and a status.

Two things worth knowing:

- **A plain (synchronous) pipe handle cannot be read and written from two threads at once**: a read that waits blocks every
  write on the same handle. The example only answers what it is sent. An extension that must send on its own schedule, for
  example after polling a server, should open the pipe with `FILE_FLAG_OVERLAPPED` and use overlapped reads and writes, or
  keep one thread that does both with `PeekNamedPipe`. The notch itself uses overlapped I/O.
- The notch drops messages above 10 per second, silently. Send a whole page in one message, and do not send a `status`
  after every message.

## 8. Security: what to expect from an extension

**An extension is a program.** The notch starts its `exe` with your rights, the same as any program you install. It can
read your files and use the network. Nothing in the notch can make that safe, so the first rule is about trust: **switch on
only extensions from authors you trust**, the way you would run any program from them. Look at the path in
*Settings > Extensions* and where it came from.

### What the notch does to protect you

| What | How |
|---|---|
| **Nothing runs by itself** | An extension is off until you switch it on in *Settings > Extensions*. Putting a manifest in the folder only makes it appear in the list. The notch never downloads or installs an extension. |
| **No other programs** | Each extension runs in its own Windows Job Object with a limit of one process: it **cannot start other programs** (no `cmd`, no `powershell`, no scripts). It is put in the job before it runs a single instruction, so there is no moment in which it could start something first. A manifest can say `"processes": "allow"` for an extension that needs helper programs; then the list says so and you decide. |
| **It ends with the notch** | The job also ends the program when the notch lets go of it, including when the notch is killed. |
| **Only you can talk to the notch** | The pipe lets in your Windows user and nobody else, not other users and not the network. The program that connects must be the `exe` of an extension you switched on (its path is checked from the connection itself). |
| **What it shows is only text** | Everything an extension sends is untrusted: text, clipped to length, without control or direction characters, never HTML or code. A picture (a `hero` image, a `matrix`) is decoded by the notch with size limits. Links open only as the result of your click on that button, and only `http` or `https` without credentials. Copying to the clipboard likewise. |
| **You can take it out** | **Remove** in *Settings > Extensions* stops it and deletes its manifest folder. |

### What the notch does not do

- It does **not** sandbox the program: it can still read and write what you can and use the network. (An AppContainer sandbox
  is a possible later step.)
- It does **not** check who made it. There is no signature or hash check yet, and a program that changes after you switched it on
  is run as it is. (Asking you to confirm a program, with its publisher and hash, and refusing one that changed, is a possible
  later step.)
- **Remove** does not uninstall the program itself. What an installer put outside the manifest folder stays until its own
  uninstaller removes it.

### For authors

Do not ask for more than you need, send plain text, and keep your program free of helper programs; an extension that needs
`"processes": "allow"` should say why in its description.

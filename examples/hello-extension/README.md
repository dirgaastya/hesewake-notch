# hello-extension

A small extension for Hesewake Notch that shows every part of the protocol in `docs/extensions.md`: a slide with filter
tabs and rows, a detail page with Open / Copy / More buttons, a second page with sections and a stepper, a toast, a badge
and a status line.

## Try it

1. Build it: `cargo build --release` in this folder.
2. Make the folder `%APPDATA%\hesewake-notch\extensions\hello-extension` and put an `extension.json` in it, with the path
   of the exe you built:

   ```json
   {
     "id": "hello-extension",
     "name": "Hello",
     "version": "0.1.0",
     "exe": "C:\\path\\to\\hesewake-notch\\examples\\hello-extension\\target\\release\\hello-extension.exe",
     "slide": true,
     "proto": 1
   }
   ```

3. In the notch, open *Settings > Extensions*, press **Rescan** and switch **Hello** on. Its slide is added to the
   carousel. **Notify** sends a toast and puts a dot on the slide's navigation dot.

The notch starts the exe itself and passes the pipe's name in `HESEWAKE_NOTCH_PIPE`.

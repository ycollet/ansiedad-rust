# ANSIedad (Rust)

A native Rust terminal port of [gabochi/ANSIedad](https://github.com/gabochi/ANSIedad),
a live-coding visual instrument inspired by bytebeat and IBNIZ. No GUI or audio
device is needed: the output is a grid of CP437-style Unicode characters with
256-color ANSI foregrounds and backgrounds.

## Run

Install a current stable Rust toolchain, then:

```sh
cargo run --release
```

To request a startup terminal size in columns by rows:

```sh
ansiedad --size 120x40
# Or, from the repository:
cargo run --release -- --size 120x40
```

On Linux/macOS, display the current console dimensions with:

```sh
stty size
```

The output is **rows columns**, the reverse of `--size COLSxROWS`.
For example, `40 120` corresponds to `--size 120x40`.

This requests a resize of the terminal window, including both panes, not just
the visual grid. Some terminal emulators disable or ignore resize requests;
the interface always uses the actual available size and supports manual resizing.
On exit, the previous terminal size is requested again. Dimensions must be
1..65535 columns and 4..65535 rows. `--check` and `--init` do not resize the terminal.
The `w` and `l` settings separately control the visual grid dimensions.

To fill the upper visual pane using the current console dimensions:

```sh
ansiedad --maximize
```

`--maximize` overrides `w` and `l` for this session, without changing your file
or resizing the terminal window. The lower editor pane remains available, and
the visuals automatically adapt to console resizing. It can also be combined
with `--size`, for example `ansiedad --size 120x40 --maximize`.

No configuration file is required, even when running the installed executable
outside this repository. By default, the editor starts with built-in shortcut
comments and a simple expression. No files are read or created automatically.
Ctrl-S creates `editme.txt` in the current directory; an existing file is never
overwritten by a scratch buffer. To open an existing file, pass its path explicitly.
The filename extension is optional: `ansiedad editme` and `ansiedad patch.txt`
work just like `ansiedad editme.txt`. These are plain text files; their contents
use the assignment format described below, not TOML.

The upper half renders visuals; the lower half is an interactive text editor,
initially focused. Type to preview valid changes
immediately, without resetting time. Invalid drafts leave the last valid
configuration running and display an error on the bottom line. Evaluation
errors leave the previous frame visible, report the expression and time, and
advance time so a temporary division by zero does not permanently stall playback.

| Key | Action |
| --- | --- |
| Tab | Switch focus between the editor and visuals |
| Ctrl-S | Save to the loaded file, or create `editme.txt` from a scratch buffer |
| Ctrl-P / Ctrl-R | Pause/resume / reset time from either pane |
| Ctrl-Q | Quit from either pane; repeat to discard an unsaved draft |
| F5 | Reload a loaded/saved file; repeat to discard an unsaved draft |
| Arrows, Home/End, Ctrl-Home/End, PageUp/Down | Navigate the editor |
| Shift + navigation keys | Extend or shrink the editor selection |
| Ctrl-Left/Right; Ctrl-Up/Down | Move by word; move by paragraph (blank-line-separated blocks) |
| Ctrl-Shift + arrows; Ctrl-Shift-Home/End | Select by word/paragraph; select to the start/end of the document |
| Ctrl-X / Ctrl-C / Ctrl-V (editor focus) | Cut / copy / paste using the system clipboard |
| Alt-M (editor focus) | Toggle `#` comments on the current line or selected lines |
| Enter, Backspace, Delete | Edit lines; multiline paste is supported |
| Space / r / q / Esc (visual focus only) | Pause / reset / quit / quit |
| Ctrl-C (visual focus only) | Quit; repeat to discard an unsaved draft |
| Esc (editor focus) | Return focus to the visuals |

The editor scrolls vertically and horizontally to keep the cursor visible.
Selected text is highlighted; typing or pasting replaces it, and Backspace/Delete
removes it. Cut and copy require a selection; without one, the status line reports
"No text selected". Clipboard shortcuts use the system clipboard on the machine
running ANSIedad. If it is unavailable (for example, in a headless SSH session),
an error is shown; a failed cut does not delete the selection.

Alt-M preserves indentation and skips blank lines. It uncomments the affected
lines when all are commented, otherwise comments them. A selection ending at
column one does not include that final line. Alt-M is used instead of Ctrl-M
because terminals commonly send Ctrl-M as Enter.

An asterisk in its header marks unsaved changes. Playback keys such as `q`,
`r`, and Space insert text while editing rather than controlling playback.
Ctrl-S rejects invalid drafts and refuses to overwrite a file changed externally.
Once a file is loaded or saved, external edits reload within 250 ms when there
is no unsaved draft.
Normal exit and Rust unwinding restore the terminal.
Use a UTF-8 terminal and a monospace font with box-drawing/block glyphs.
Your terminal's palette controls the appearance of ANSI colors 0 through 15.

```sh
cargo run --release -- my-patch.txt
cargo run -- --init                # optional: create editme.txt
cargo run -- --init my-patch.txt  # create an example; never overwrite
cargo run -- --check my-patch.txt
cargo install --path .
ansiedad --help
```

`--check` validates the built-in example when no path is supplied, or the named
file otherwise. Missing or invalid explicit files are reported as errors.
It validates syntax and settings, not every possible time-dependent
evaluation. `--frames N` runs a finite number of frames in an interactive terminal.

## Graphical application (optional)

`ansiedad-gui` is a separate egui/eframe application with a live text editor and
a graphical canvas. The console executable, its patch syntax, and the default
build remain unchanged. GUI dependencies are enabled only with the `gui` feature.
A desktop session and an OpenGL-capable graphics driver are required to open
the window; `--check` and `--init` do not require a display.

```sh
cargo run --release --features gui --bin ansiedad-gui
cargo run --release --features gui --bin ansiedad-gui -- examples/pixels.txt
cargo run --release --features gui --bin ansiedad-gui -- examples/characters.txt
cargo run --features gui --bin ansiedad-gui -- --check examples/pixels.txt
cargo run --features gui --bin ansiedad-gui -- --init canvas.txt
cargo install --path . --features gui --bin ansiedad-gui
ansiedad-gui --help
```

The GUI uses the same plain-text assignments, comments, constants, operators,
string indexing, `ord()`, and `len()`. Existing console patches open in character
mode by default. These additional names are reserved **only in graphical patches**:

| Name | Type / default | Meaning |
| --- | --- | --- |
| `mode` | Unquoted integer, `0` | `0`: character grid using `b`, `f`, and `c`; `1`: pixel canvas using `rgba`. |
| `rgba` | Quoted integer expression, `"0x000000ff"` | Packed `0xRRGGBBAA` color; the low 32 bits are used. `AA = ff` is opaque; `AA = 00` is transparent. |
| `x`, `y` | Read-only integers | Zero-based column and row of the current character or pixel. |
| `frame` | Read-only integer | Animation frame counter, starting at zero and increasing once per rendered frame. |

`w` and `l` are character columns/rows in mode 0 and pixel width/height in mode 1,
each `1..512`. `t` retains its console meaning: it advances once per sample in
row-major order, while `frame` is shared by every sample in a frame. Pause freezes
both counters; reset sets both to zero. `fps` and `s` control frame timing as in
the console. Console scrolling (`m`) has no effect in the graphical window.
Character mode uses the original glyph table with a fixed xterm-style 256-color
palette rather than the terminal emulator's palette.

For translucent animated pixels:

```python
mode = 1
w = 160; l = 100; fps = 30
rgba = "(((x*2+frame)&255)<<24)|(((y*2+frame)&255)<<16)|(((x^y^frame)&255)<<8)|192"
```

To assemble the four 8-bit channels of `0xRRGGBBAA` dynamically:

```python
rgba = "((x&255)<<24) | ((y&255)<<16) | (128<<8) | 255"
```

This uses `x` for red and `y` for green (each masked to `0..255`), a fixed blue
value of `128`, and full opacity (`255`). Shift red by 24 bits, green by 16,
and blue by 8; alpha occupies the lowest 8 bits. Combine the channels with `|`.

Pixels are scaled with nearest-neighbor filtering, with integer scaling when
enlarged. Alpha blends against the optional checkerboard (or black when disabled);
it does not accumulate previous frames or create motion trails.

Edit `mode` in the patch to switch renderers; choose suitable `w`/`l` values for
the new mode. Editor changes apply automatically. Invalid drafts keep the last
valid configuration running; evaluation errors keep the last complete frame
visible and advance time so transient errors can recover.

| GUI shortcut | Action |
| --- | --- |
| Shift + arrows / Home / End | Extend the text selection |
| Ctrl + Left / Right, Ctrl + Up / Down | Jump by word or paragraph; add Shift to select |
| Ctrl + Home / End | Jump to the start/end of the patch; add Shift to select |
| Ctrl-X / Ctrl-C / Ctrl-V | Cut / copy / paste the selection using the system clipboard |
| Alt-M | Toggle `#` comments on the current line or selected lines |
| Ctrl-S | Save |
| F5 | Reload the currently loaded file |
| Ctrl-P / Ctrl-R | Pause/resume / reset time |
| Ctrl-Q | Quit, with confirmation for unsaved changes |

Selection, navigation, clipboard, and comment shortcuts act in the patch editor.
Native platform shortcuts remain available, including Cmd-X/C/V/S and Cmd-Z /
Cmd-Shift-Z on macOS, and Ctrl-Z / Ctrl-Shift-Z for undo/redo on other platforms.
Alt-M preserves indentation and skips blank lines, just as in the console.

Enter a path and use **Open / reload** or **Save** (`Ctrl-S`, or `Cmd-S` on macOS).
Saving rejects invalid drafts, refuses to overwrite a new path, and checks loaded
files for external changes. Opening a file or closing the window requires
confirmation when the draft is modified. Reloading is manual in the GUI; F5
reloads the loaded file even if you have edited the Path field.
`--frames N` closes the window after a finite number of frames.

### Video export and live recording

Install **FFmpeg with the `libx264` encoder** and put `ffmpeg` on `PATH`
(for example, `brew install ffmpeg` on macOS or `sudo apt install ffmpeg` on
Debian/Ubuntu). It is a runtime dependency only: neither building nor normal
playback requires FFmpeg, and release installers do not bundle it.

Expand **Video export / recording** in the GUI, enter a new `.mp4` path,
duration, and frame rate, then choose:

- **Export from frame 0**: render a snapshot of the current draft offline,
  starting at `t = 0`, `frame = 0`. Edits and playback controls do not affect
  this export. Invalid patches or evaluation errors stop the export.
- **Start recording**, then **Stop and save**: record the current canvas in
  real time, including live edits, pause/resume, and time resets. Paused frames
  repeat; an invalid draft or evaluation error retains the last valid image,
  just as in the preview. Changing the canvas dimensions cancels recording.

Both operations run in a background worker with a frame counter and a cancel
button. Stop/save or cancel the video before closing the application. If a
recording's bounded encoding queue fills, recording is canceled with an error
instead of silently dropping frames. Existing files are never overwritten;
failed or canceled exports remove their temporary files.

For deterministic **headless export**, without a window or desktop session:

```sh
cargo run --release --features gui --bin ansiedad-gui -- \
  --export animation.mp4 --seconds 10 --video-fps 30 examples/pixels.txt
ansiedad-gui --export characters.mp4 --seconds 5 examples/characters.txt
```

Omit the patch path to export the built-in animation. Duration defaults to
10 seconds and the export frame rate defaults to the patch's `fps` (1..240).
Offline export produces `ceil(seconds * fps)` frames, advancing `frame` once
and `t` by `w * l` per output frame; it does not wait for the patch's `s` delay.
`--seconds` and `--video-fps` require `--export`, which cannot be combined with
`--check`, `--init`, or `--frames`.

Videos contain only the canvas, without editor controls or audio. Pixel mode
uses the native `w` by `l` resolution; character mode uses 8 by 16 pixel cells
and the GUI's bundled monospace fonts. Transparent pixels composite onto black
(the preview checkerboard is not recorded). Odd output dimensions are padded
with black to even dimensions for H.264/YUV420 compatibility.

## Live expressions

Patch files contain `name = value` assignments separated by newlines or `;`.
`#` starts a comment through the end of the line. Separators and comment markers
inside quoted strings are ordinary characters. Use single or double quotes
around `b`, `f`, and `c` expressions and string constants; numeric settings and
integer constants are unquoted. Both quote styles support the escapes `\\`,
`\'`, `\"`, `\n`, `\r`, and `\t`. Strings cannot span physical lines.

Patch errors include **1-based line and column numbers**, counting Unicode
characters rather than UTF-8 bytes, for example:
`line 2, column 12: b: unknown constant missing`.
Expression syntax errors point into the original quoted text, including escapes.
Runtime errors identify the start of the affected expression and retain the
current `t` (plus `x`, `y`, and `frame` in the GUI).

The built-in starter is deliberately simple:

```python
b = "t >> 4"
f = "15"
c = "144"
```

### Reserved settings and built-ins

These names configure the renderer, so they cannot be used for custom constants.
All settings are optional. The defaults below apply when an assignment is omitted;
the built-in starter above explicitly overrides `b`, `f`, and `c`.

| Name | Expected type | Default | Meaning and allowed values |
| --- | --- | --- | --- |
| `b` | Quoted integer expression | `"0"` | Background color; the result is reduced modulo 256 to an index in `0..255`. |
| `f` | Quoted integer expression | `"7"` | Foreground color; the result is reduced modulo 256 to an index in `0..255`. |
| `c` | Quoted integer or character expression | `"0"` | Integers select a character-table index modulo 224; characters such as `test[1]` are displayed directly. |
| `w` | Unquoted integer | `64` | Grid width in columns, `1..512`; clipped to the available visual pane. |
| `l` | Unquoted integer | `16` | Grid height in rows, `1..512`; clipped to the available visual pane. |
| `m` | Unquoted integer | `0` | Rendering mode: `0` redraws the grid; `1` scrolls within the visual pane. |
| `s` | Unquoted number, integer or floating-point | `0.0` | Finite delay in seconds per visible row, `0..1`; batched into a delay per frame. |
| `fps` | Unquoted integer | `30` | Maximum frame rate in frames per second, `1..240`; `s` may slow playback further. |

For example, write `b = "t >> 4"` but `w = 64` and `s = 0.01`.
Numeric settings take literal values, not expressions such as `w = 32*2`.

Three other names are reserved inside expressions: `t` is the read-only integer
cell counter, starting at zero; `ord(...)` returns a single character's integer
Unicode code point; and `len(...)` returns a string's integer character count.
None can be assigned in a patch file. Other valid names define custom integer
constants, string constants, or integer expression bindings.

### User-defined expressions

Use unquoted expressions to name intermediate calculations. They are evaluated
with the current `t` (and `x`, `y`, `frame` in the GUI), not frozen at assignment:

```python
mode = 1
rval = x & 255
gval = y & 255
bval = (x ^ y) & 255
aval = (x | y) & 255
rgba = "rval << 24 | gval << 16 | bval << 8 | aval"
```

Bindings can refer to other bindings defined before or after them. Circular
references and unknown names are errors. Bindings must produce integers and
also work in `b`, `f`, and `c` expressions in the console editor. Quoted user
values remain literal strings: `TEXT = "Hello"` is text, while
`shade = t & 255` is an expression. Renderer settings such as `w` and `fps`
still require literal values.

### More examples

For the original grayscale/block example, replace the editor contents with:

```python
b = "(t*t>>17)%16+240"     # background, modulo 256
f = "(t*t>>16)%16+240"     # foreground, modulo 256
c = "144+(t^(t>>8))%4"    # character index, modulo 224
w = 64                   # columns, 1..512
l = 16                   # rows, 1..512
m = 0                    # 0: redraw; 1: scroll
s = 0.0                  # row delay, 0..1 seconds; batched per frame
fps = 30                 # maximum frame rate, 1..240
```

For colorful bitwise patterns:

```python
b = "t<<5 ^ t>>1"
f = "0"
c = "0"
```

For monochrome character patterns:

```python
b = "7"
f = "0"
c = "222*((t*t>>(9+t%2))%2)+(188+(t>>(t>>10)%15)%4)"
```

For the original bitmap and scrolling-text example:

```python
BYE = "ANSIedad...."; BITMAP = 0b0001001101010101000100100101011100000001010101010001001101100010
b = "BITMAP >> ((t>>1)+(t>>10))%16 + (t>>7)%4*16 & 1 ^ (t>>9)%2 * t<<2"
f = "t&1"; c = "(t%512<64)*(ord(BYE[t%12])-32)"
```

Declare integer or string constants directly alongside the settings; no
`[constants]` section is needed or accepted. Constants may appear before or after
the expressions that reference them. Names use ASCII letters, digits, and
underscores, cannot start with a digit, and cannot be `t`, `ord`, or `len`. Setting names
(`b`, `f`, `c`, `w`, `l`, `m`, `s`, `fps`) are reserved for their settings.
Names are case-sensitive. Remove leading `#` to enable commented-out examples.

Use one set of `b`, `f`, and `c` assignments at a time; duplicate assignments
are invalid, including constants. Dimensions and playback settings are optional.
Existing simple assignment-only TOML files still work, regardless of extension;
move any old `[constants]` entries to the top level and remove that heading.

`t` starts at zero and increases once per rendered cell in row-major order.
The grid is clipped to the upper pane, reserving the lower pane for editing
and the bottom row for status. Scroll mode is confined to the upper pane;
only visible cells advance time. Playback is capped at `fps`, or slowed further
by `s * visible_rows` seconds per frame. Pausing freezes time, but valid edits
still refresh the preview.

Expressions support `t`, decimal/hex/binary/octal integers, parentheses, unary
`+ - ~`, and `+ - * / // % ** << >> & ^ | == != < <= > >=`.
Operator precedence follows Python's arithmetic/bitwise rules; power is
right-associative. Comparisons produce 0 or 1; use individual comparisons
instead of Python-style comparison chains.

### Strings and characters

String indexing works outside `ord()`: `test[1]` selects the second character.
Use it directly in `c` to display that character, without subtracting 32:

```python
test = "TEST"
c = "test[t % len(test)]"
```

`len(test)` returns `4`. It also accepts string literals, empty strings
(`len('')` is `0`), and indexed characters (`len(test[1])` is `1`).
Length and indexing count Unicode characters, not UTF-8 bytes or visual columns.
Indices start at zero, support Python-style negative indices (`test[-1]` is the
last character), and report out-of-range indices as evaluation errors.

`c` also accepts a single-character string literal or constant, such as
`c = "'E'"`. Direct characters must be printable and occupy exactly one terminal
column; control characters, combining marks, and wide characters are rejected
to preserve the grid layout. Numeric `c` expressions still use the original
224-character table.

`ord('A')` returns `65`; `ord(test[1])` returns `69` for `"TEST"`.
Use `ord()` when doing arithmetic with a character or using it in `b` or `f`:
characters are not implicitly converted to integers. `ord()` requires exactly
one character. Expression string
literals accept single or double quotes and the escapes `\\`, `\'`, `\"`,
`\n`, `\r`, and `\t`; remember to escape them for the outer assignment string too.

Unlike the original's arbitrary Python `eval`, expressions cannot execute
code, access files, or call functions other than `ord` and `len`. Custom values
must be integer or string constants; arithmetic operators require integers.
The assignment format is a limited Python-like syntax, not a Python interpreter.
Both `/` and `//` perform integer
floor division, `%` follows Python's signed modulo, and arithmetic wraps at
signed 64 bits instead of using Python's unlimited integers. Negative shifts,
division by zero, and exponents outside `0..4294967295` are reported as errors.
Shifts of 64 or more produce zero (or -1 for a negative arithmetic right shift).
Expressions are limited to 4096 bytes and 64 parser nesting levels.

The original 224-character table is preserved, including its repeated tilde
and final square. Character 0 is a space, 144 is light shade, and 187 is a
solid block. For color-only visuals set `c = "0"` and animate `b`.

## Development

GitHub Actions checks both applications on Linux, macOS, and Windows and builds
native release installers. See the [CI and release procedure](.github/README.md)
for manual builds and publishing version-tagged releases.

The application icon source is `assets/ansiedad.svg`. After editing it, run
`bash scripts/generate-icons.sh` on macOS with ImageMagick installed to regenerate
the checked-in PNG, Windows ICO, and macOS ICNS assets.

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
# Include the optional graphical application:
cargo test --features gui
cargo clippy --features gui --all-targets -- -D warnings
```

## Attribution and license

Based on **ANSIedad by gabochi**, upstream revision
`a2039fcb3e33b6be835fe1967f736d613d2076bb`. The original rendering behavior,
character mapping, and example expressions inform this port. Bytebeat was
pioneered by [viznut](http://viznut.fi/en/).

The preview-above-controls layout is also inspired by
[abnuo/ansiedad-editor](https://github.com/abnuo/ansiedad-editor).

Distributed under the GNU Affero General Public License v3.0; see [LICENSE](LICENSE).

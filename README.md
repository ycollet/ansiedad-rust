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

## Live expressions

Patch files contain `name = value` assignments separated by newlines or `;`.
`#` starts a comment through the end of the line. Separators and comment markers
inside quoted strings are ordinary characters. Use single or double quotes
around `b`, `f`, and `c` expressions and string constants; numeric settings and
integer constants are unquoted. Both quote styles support the escapes `\\`,
`\'`, `\"`, `\n`, `\r`, and `\t`. Strings cannot span physical lines.

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
| `c` | Quoted integer expression | `"0"` | Character-table index; the result is reduced modulo 224 to an index in `0..223`. |
| `w` | Unquoted integer | `64` | Grid width in columns, `1..512`; clipped to the available visual pane. |
| `l` | Unquoted integer | `16` | Grid height in rows, `1..512`; clipped to the available visual pane. |
| `m` | Unquoted integer | `0` | Rendering mode: `0` redraws the grid; `1` scrolls within the visual pane. |
| `s` | Unquoted number, integer or floating-point | `0.0` | Finite delay in seconds per visible row, `0..1`; batched into a delay per frame. |
| `fps` | Unquoted integer | `30` | Maximum frame rate in frames per second, `1..240`; `s` may slow playback further. |

For example, write `b = "t >> 4"` but `w = 64` and `s = 0.01`.
Numeric settings take literal values, not expressions such as `w = 32*2`.

Two other names are reserved inside expressions: `t` is the read-only integer
cell counter, starting at zero, and `ord(...)` is the built-in function that
returns a single character's integer Unicode code point. Neither can be assigned
in a patch file. Other valid names define custom integer or string constants.

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
underscores, cannot start with a digit, and cannot be `t` or `ord`. Setting names
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

`ord('A')` returns a character's Unicode code point. String literals or string
constants can be indexed inside `ord`, as in `ord("abc"[t%3])` or
`ord(BYE[t%12])`. Indexing counts Unicode characters, supports Python-style
negative indices, and reports out-of-range indices as evaluation errors.
Without an index, `ord` requires exactly one character. Expression string
literals accept single or double quotes and the escapes `\\`, `\'`, `\"`,
`\n`, `\r`, and `\t`; remember to escape them for the outer assignment string too.

Unlike the original's arbitrary Python `eval`, expressions cannot execute
code, access files, or call functions other than `ord`. Custom values must be
integer or string constants; strings are only supported as `ord` arguments.
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

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## Attribution and license

Based on **ANSIedad by gabochi**, upstream revision
`a2039fcb3e33b6be835fe1967f736d613d2076bb`. The original rendering behavior,
character mapping, and example expressions inform this port. Bytebeat was
pioneered by [viznut](http://viznut.fi/en/).

The preview-above-controls layout is also inspired by
[abnuo/ansiedad-editor](https://github.com/abnuo/ansiedad-editor).

Distributed under the GNU Affero General Public License v3.0; see [LICENSE](LICENSE).

# CLI reference

See the [project README](../README.md) for installation and quick start. Run `agent-latex demo` for a formula gallery.

## CLI

### Render one expression

```bash
agent-latex render '\int_0^1 x^2\,dx = \frac{1}{3}'
agent-latex render --inline 'x^{n+1}'
agent-latex render --file formula.tex
cat formula.tex | agent-latex render
```

Useful flags:

| Flag | Meaning |
|---|---|
| `--inline` | compact single-line form |
| `--width N` | maximum line width; wide display formulas use wrapped compact text |
| `--theme auto\|mono\|ansi\|dark\|light` | ANSI theme for 2D block art; inline math is left unstyled |
| `--no-color` | disable ANSI styles |
| `--indent N` | indent every art row |
| `--center` | center the art in the target width |
| `--json` | machine-readable result and diagnostics |

### Transform Markdown

```bash
agent-latex md notes.md
cat examples/math.md | agent-latex filter
```

`md` and `filter`:

- scan `$...$`, `\(...\)`, `$$...$$`, `\[...\]`, and fenced `latex`/`math`/`tex`
  blocks;
- skip fenced, indented and inline code;
- reject currency-like `$5 and $10`;
- wrap block art in a `text` code fence by default, so Markdown renderers keep
  the 2D whitespace intact;
- use `--plain` to emit raw art for a plain terminal.

Example:

````markdown
The equation is

```text
 a
───
 b
```

and inline x².
````

### Live stream

```bash
# Any CLI that writes Markdown/text to stdout:
my-agent ... | agent-latex live

# Or feed a file:
agent-latex live < conversation.md
```

On a TTY, an open formula is rendered as a replaceable preview and repainted as
tokens arrive; when the closing delimiter arrives it is committed as stable
output. On a pipe (no TTY), previews are skipped and only committed math is
rendered, so the output stays append-only. If the stream ends, or a line break
interrupts an inline formula, before its closing delimiter arrives, the
original text is emitted unchanged; no `$` or fence is invented.

`live` reads plain text or Markdown. Decode JSON/JSONL event streams to text before passing them to `live`.

### Interactive wrapper

```bash
agent-latex wrap -- opencode
agent-latex wrap -- claude

# or: alias oc='agent-latex wrap -- opencode'
#     alias cc='agent-latex wrap -- claude'
```

Runs the existing TUI in a PTY, parses its virtual screen, and renders math
**in place**. The child's input, session and internal buffer are untouched; only
the displayed cells change. The same compositor handles OpenCode's bordered
composer and Claude Code's `❯` prompt.

Platforms: Linux and macOS.

Hotkey:

```bash
agent-latex config set toggle-key ctrl+g
agent-latex config get toggle-key
agent-latex config path

agent-latex wrap -- opencode   # uses the saved key
```

The setting is stored in `$XDG_CONFIG_HOME/agent-latex/config.json`, or
`~/.config/agent-latex/config.json`. Accepted values are `f1`..`f12`,
`ctrl+<letter>`, `alt+<letter>`, or a raw escape sequence. The default is
`f12`. On macOS, F12 may be captured by the system; `ctrl+g` is a safe
alternative.

`--toggle-key KEY` overrides the saved value for one run; `--no-hotkeys`
disables the toggle entirely.

Math detection is conservative for generic commands and aggressive for
`opencode`/`claude`: generic output only accepts `$$...$$`, `\[...\]`,
`\(...\)`, and `$...$` that contains an unmistakable math token. This avoids
rendering `$HOME`, `$5`, or shell variables. Agent prose still allows math next
to operators, punctuation, borders and non-ASCII text (for example `$x$>0`), but
a delimiter stuck to an ASCII word or shell assignment (`PATH=$HOME:$PATH`) is
left alone.

Two rendering policies:

- **Inline math in prose** uses the compact one-line form (`x²`, `a/b`,
  `∫₀¹`). If the normal spaced form is wider than the original `$...$` span, a
  compact no-space form is used instead of leaving the source unrendered. The
  rest of the line shifts left, so there is no trailing gap.
- **Standalone formulas and display math** use the full 2D renderer. In the
  conversation area the compositor reserves the formula's full height, pushes
  later lines down, and scrolls older lines out above the fixed input/status
  area. This keeps display math stable instead of switching between `a/b` and a
  vertical fraction as the conversation grows.

Rendering examples:

- `The value is $x^2$ today.` renders as `The value is x² today.` with no gap;
- `$a=1, b=4$` and `$0.25$` both render (decimals are math, not currency);
- a standalone `$\frac{a}{b}$` renders as a vertical fraction;
- `$$\int_0^1 x\,dx$$` renders with a tall integral sign and aligned limits;
- `\xrightarrow{n\to\infty}` renders as an arrow with the label above it.

Terminal behavior:

- full-screen TUIs only; the wrapper uses the alternate screen;
- inline fractions/integrals stay one line by design (a terminal line cannot
  grow); only standalone/display math gets the 2D form;
- display reflow reserves rows above the fixed input/status area; free rows at
  the bottom are used first, and only then do older rows scroll out of the top.
  Reflow is disabled while the transcript is scrolled up (`Jump to latest`);
- the input/status boundary is detected from the composer's border; when the
  border is not visible nothing is hidden, so conversation math is not skipped;
- inline math in the editable composer renders; multi-line display math stays
  raw while the cursor is inside it, so editing and line breaks are not split;
- the toggle hotkey is F12 by default; use `--toggle-key` to change it or
  `--no-hotkeys` to disable it;
- a lone Escape is separated from a split function-key sequence by a 50 ms
  input timeout;
- window resizing is forwarded to the child. Keyboard navigation is the
  supported interaction mode; mouse reporting and advanced keyboard protocols
  are not forwarded.

Press **F12** to toggle between rendered math and the original terminal output.
The agent's own copy command still copies the raw session text; terminal
selection copies the displayed cells, so F12 is the quickest way to inspect or
copy the original LaTeX. Use `--no-hotkeys` to disable the toggle.

Use `--no-inline` to run the PTY layer without replacement for debugging. It
also disables the toggle hotkey, so a stray key cannot turn replacement back on.

### Diagnostics

```bash
agent-latex doctor
```

Prints terminal size, color support, theme state and two sample renderings.

## Supported TeX subset

- Greek letters, common operators, relations, arrows, set/logic symbols;
- superscripts and subscripts, including primes;
- `\frac`, `\dfrac`, `\tfrac`, `\binom`;
- `\sqrt` and `\sqrt[n]`;
- `\left...\right`, auto-scaled delimiters;
- `matrix`, `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`;
- `cases`, `aligned`, `align`, `gathered`, `split`, `array`;
- `\text`, `\mathrm`, `\mathbf`, `\mathit`, `\mathbb`, `\mathcal`, `\mathfrak`;
- accents: `\hat`, `\bar`, `\vec`, `\dot`, `\ddot`, `\tilde`, `\overline`,
  `\underline`, `\check`, `\breve`, `\acute`, `\grave`;
- big operators with limits: `\sum`, `\prod`, `\int`, `\oint`, `\bigcup`, ...;
- functions: `\sin`, `\cos`, `\log`, `\lim`, `\max`, `\det`, ...;
- explicit spacing: `\,`, `\:`, `\;`, `\quad`, `\qquad`, `\!`.

Simple scripts use Unicode forms such as `x²` and `xᵢ`. Compound script contents
use compact linear expressions to keep nested exponents and indices together.
Sums and integrals retain their two-dimensional limit placement.

Unknown commands are preserved as error nodes with diagnostics; they do not
abort the rest of the formula.

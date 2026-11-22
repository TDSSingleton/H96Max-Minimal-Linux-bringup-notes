# 14. How EBI draws Arabic (and everything else) on a text console

Companion to NOTES §43. Code: `tools/h96ui-rs/src/fb.rs` (pixels) and `src/text.rs` (widths,
shaping, right-to-left). Every example below is pinned by a unit test in `text.rs`.

Drawing Arabic correctly needs three separate things, and the Linux console has none of them:

1. **Glyphs**: pictures for the letters at all.
2. **Shaping**: an Arabic letter changes shape depending on whether it connects to its
   neighbours.
3. **Bidirectional ordering**: Arabic and Hebrew are read right to left, inside a screen
   that EBI lays out left to right.

## 1. Glyphs: why the console could not, and what replaced it

The kernel console (fbcon) draws from a console font of at most **512 glyphs**. Terminus, the
font we loaded, covers Latin, Greek and Cyrillic. Arabic needs a few hundred more shapes (see
§2), Japanese and Chinese need thousands. No console font can do it, so EBI stopped using the
console for drawing:

- tty1 is switched to **`KD_GRAPHICS`** (`ioctl(tty, KDSETMODE, 1)`). fbcon stops painting,
  but keys still arrive on the tty, so input does not change.
- `/dev/fb0` (1920×1080, 32 bits per pixel, one row = 7680 bytes) is `mmap`ed. Writing a `u32`
  into it changes a pixel on the TV.
- EBI still produces exactly the same text it wrote to the console (colour codes, "go to row
  r column c", "clear to end of line"). A ~100-line interpreter turns that into a 120×33 grid
  of cells (character + colours), and only rows that changed are drawn again.

The pictures come from **GNU Unifont** (`/usr/share/unifont/unifont.hex`, Alpine package
`unifont-misc`): one line per character, a code point and the bitmap in hex. An 8×16 glyph is
32 hex digits (one byte per pixel row), a 16×16 (wide) glyph is 64. For example (the shape
of an "A" in this format):

```
0041:0000000018242442427E424242420000

00 ........      42 .#....#.
00 ........      42 .#....#.
00 ........      7E .######.
00 ........      42 .#....#.
18 ...##...      42 .#....#.
24 ..#..#..      42 .#....#.
24 ..#..#..      00 ........
42 .#....#.      00 ........
```

Each glyph pixel becomes 2×2 screen pixels, so an 8×16 glyph fills one 16×32 cell, the same
cell size as the old console font (120×33 cells on 1080p). Japanese and Chinese glyphs are
16×16 and take two cells. Bit set = foreground colour, bit clear = background, one `u32` per
pixel straight into the framebuffer. That is the whole renderer, and it does not care which
script a glyph belongs to. The hard part of Arabic is choosing *which* glyph to draw, and
*where*.

## 2. Shaping: four forms per letter

Strings store Arabic in **logical order** (the order it is read and typed) using the basic
letters of the Arabic block, U+0600–U+06FF. One code point per letter, whatever its
position: م (meem) is always U+0645.

On paper, though, a letter is written in up to four forms, depending on whether it connects to
the letter before it and to the letter after it:

| connects to previous? | connects to next? | form |
|---|---|---|
| no | no | isolated |
| yes | no | final |
| no | yes | initial |
| yes | yes | medial |

Real systems choose and draw these shapes with an OpenType font and a shaping engine
(HarfBuzz). We have neither. But Unicode kept an old compatibility block, **Arabic
Presentation Forms-B (U+FE70–U+FEFF)**, with every form of every basic letter as its own
code point, laid out in a fixed pattern: isolated, final, initial, medial at +0, +1, +2, +3.
Meem's isolated form is U+FEE1, so its final is U+FEE2, initial U+FEE3, medial U+FEE4. Unifont
has glyphs for all of them. So shaping can become a lookup plus an addition:

```
presentation form = isolated form of the letter + (0 | 1 | 2 | 3)
```

`text.rs` has one table row per letter: its isolated presentation form, and its **joining
type**:

- **Dual-joining** (most letters: ب beh, م meem, ح hah, س seen, ل lam …): four forms.
- **Right-joining** (ا alef, د dal, ر reh, و waw, ة teh marbuta, …): they connect to the letter
  *before* them but never to the one *after*. Only isolated and final.
- **Non-joining** (ء hamza): isolated only.
- **Join-causing** (ـ tatweel, the stretching line): connects on both sides, has no forms.

Persian/Urdu letters (پ گ ک چ ژ ی) are in the same table, from Presentation Forms-A, which
uses the same layout.

The rule for each letter: it **connects to the previous** letter if it can join backwards
(dual or right) *and* the previous letter can join forwards (dual only). It **connects to
the next** letter if it can join forwards (dual) *and* the next letter can join backwards
(dual or right). Those two yes/no answers pick the form from the table above.

### Worked example: محمد (Muhammad)

Logical order: م ح م د = U+0645 U+062D U+0645 U+062F.

| letter | joining | previous joins forward? | next joins back? | form | code point |
|---|---|---|---|---|---|
| م meem | dual | (none) | ح yes | initial | U+FEE3 |
| ح hah | dual | م yes | م yes | medial | U+FEA4 |
| م meem | dual | ح yes | د yes | medial | U+FEE4 |
| د dal | right | م yes | (none) | final | U+FEAA |

### The one ligature that is not optional: lam + alef

ل (lam) followed by ا (alef, or its variants آ أ إ) is always written as one combined
shape, لا. Presentation Forms-B has it as ligatures (U+FEF5–U+FEFC, again isolated/final
pairs). So when the shaper sees lam + alef, it emits one ligature (final if the letter
before lam joins forward) and skips the alef.

سلام (salam, "peace") = س ل ا م:

- س seen: nothing before, lam after joins back → **initial** U+FEB3
- ل+ا: seen before joins forward → **lam-alef final** U+FEFC (alef consumed)
- م meem: the letter before is alef, which never joins forward → **isolated** U+FEE1

### Marks

Short vowels and other diacritics (فتحة, ضمة, شدة … U+064B–U+065F) sit *above or below* a
letter. They do not affect joining (the shaper looks past them), and EBI does not draw them:
Unifont draws a mark as a separate glyph meant to be overlaid, and our grid has one character
per cell. Channel names rarely carry them; the letters are still readable without them.

## 3. Bidirectional text: drawing right to left inside a left-to-right screen

After shaping, محمد is U+FEE3 U+FEA4 U+FEE4 U+FEAA, still in reading order. The cell grid
is filled left to right, so drawn as is, the word would come out backwards: the first letter
on the left. Arabic and Hebrew are read **right to left**, so the first letter has to be the
*rightmost*.

The full answer is the Unicode Bidirectional Algorithm (UAX #9): several pages of rules about
levels, weak and neutral characters, embeddings. EBI implements the part that matters for one
line of a menu whose overall direction is left to right (the UI's language):

1. Find a **right-to-left run**: from an Arabic/Hebrew letter up to the last Arabic/Hebrew
   letter **or number** before the next Latin letter, including the spaces and punctuation in
   between.
2. **Shape** the run (§2), then **reverse** it.
3. Inside the reversed run, put **numbers** back in reading order (digits are always written
   left to right, even inside Arabic), together with a `.` `,` `:` between digits ("1.5").
4. **Mirror** brackets: `(` and `)` swap, as do `[]`, `{}`, `<>`, `«»`. In right-to-left text
   an "opening" bracket is drawn facing the other way.
5. Everything outside runs (Latin, CJK, symbols) stays where it is.

So محمد is drawn U+FEAA U+FEE4 U+FEA4 U+FEE3, left to right on the screen: dal, meem, hah, meem,
which is محمد as an Arabic reader expects it.

Numbers after right-to-left text belong to the run. That is UBA rules W2/W7/N1: a number
whose nearest strong letter before it is Arabic or Hebrew is not Latin, and the space between
takes the direction of both sides. The first version stopped the run at the last letter and
got this wrong; the sample render in §43 showed it:

| logical (as stored) | drawn left to right |
|---|---|
| `\|AR\| قناة الجزيرة 24` | `\|AR\| 24` + the two words reversed (24 on the left, as in Arabic) |
| `כאן 11` (Kan 11, Hebrew) | `11 ןאכ` |
| `11 כאן` | `11 ןאכ` (a number *before* RTL text stays Latin) |
| `כאן 11 HD` | `11 ןאכ HD` |
| `قناة 24 (HD)` | `24 ` + قناة reversed + ` (HD)`: `(HD)` is Latin, outside the run |

Hebrew needs only steps 1–5: its letters do not join (it has five final forms, but those are
separate letters that are typed as such).

## 4. Keeping the layout intact

Menus are laid out *before* the text reaches the renderer: `fit()` pads every label so the
right-hand column lines up. Two things could change a line's width afterwards:

- **lam-alef** turns two letters into one glyph (one cell less),
- **marks** are dropped (they have zero width anyway).

`visual()` measures each run before and after and **pads it with spaces** to its original
width, so the "★ Arabic" column stays where `fit()` put it. Widths everywhere come from
`char_width()`: 0 for marks and invisible format characters (RLM, ZWJ, …), 2 for CJK and
full-width forms, 1 for the rest.

## 5. What it does not do

Fine for channel and film names. Not a typesetting engine:

- **No harakat** (vowel marks) drawn, see §2.
- **One direction per line:** the line is always left to right. A fully Arabic line comes out
  right in the visual sense (the run is reversed as a whole), but alignment is still left.
- **No explicit bidi controls** (RLE/RLI/PDF …): they are skipped as zero-width, not obeyed.
- **Only letters that have presentation forms** are joined: the Arabic alphabet, plus پ چ ژ ک
  گ ی. Rarer letters (some Urdu/Pashto/Sindhi ones) are drawn in their isolated shape.
- **Style:** Unifont is a 16-pixel bitmap font, doubled. Legible on a TV across a room, not
  pretty. Nastaliq (Urdu calligraphic style) is out of reach of any per-cell renderer.

## 6. Trying it

A sample screen (Japanese, Arabic, Hebrew, Korean, Chinese, Greek, Cyrillic) is rendered with
the real font into a raw image by an ignored test:

```bash
# on the PC, from tools/h96ui-rs; copy unifont.hex from the box or from unifont-misc
H96UI_FONT=…/unifont.hex H96UI_RENDER_OUT=…/sample.raw cargo test render_sample -- --ignored
```

On the box, a screenshot is `/dev/fb0` read back over ssh (1920×1080, XRGB, 7680 bytes per
row). `H96UI_CONSOLE=1` gives the old console renderer back for comparison.

# ez-CGER `.hcg` project file format (ez-CGER 2.10.65, file version 0x78)

This document describes the **structure and field meanings** of `.hcg` files for interoperability (rewriting image paths).

**Evidence tags**
- **[confirmed]**: verified by comparing sample bytes or on the app's screen
- **[unverified]**: there is supporting evidence, but not yet verified with samples or on screen
- **[guess]**: unverified hypothesis

**Verification:** `project/reference/hcg_parse.py` reads every hcg in `project/samples` with 100% byte coverage, and writing it back with `hcg_write.py` is byte-identical (`project/scripts/reference_roundtrip.sh`). The set includes embedded PNG/JPEG/BMP images and an empty image block left by the app after re-saving an embedded image (see `project/samples/README.md`).

All integers are little-endian. `i32` = int32, `f32` = float, `WStr` = `i32 character count + UTF-16LE` (no terminator).

## Summary

- **Structure [confirmed]:** FileHeader, DocHeader, global layer, pages, objects, in that order. The parser and writer return every sample byte for byte.
- **Object types [confirmed]:** 2 line, 3 rectangle, 4 ellipse, 5 image, 6 text. 1 freehand, 7 video, 8 PIP are [unverified].
- **Confirmed properties:** rectangle position and size, rotation (radians, positive = clockwise), line/fill colour and alpha, line width, outline, shadow, visibility (0xEA), gradient on/off and direction (0 up, 1 right, 2 down, 3 left), corner kinds (0 square, 1 cut, 2 round; order TL, TR, BR, BL) and size (f32 at 0xC4), alignment (horizontal 0 centre · 1 left · 2 right / vertical 0 middle · 1 top · 2 bottom), text scroll · wrap · line spacing · font name and size, image crop · fit · alpha, sequence (interval in ms, UI 15–100; delay stored in ms and shown in seconds), line points 12 B `{i32 kind (ignored), x, y}`.
- **Page transition [confirmed]:** header 0x24 = enabled, 0x28 = type (0 Cut, 1 Mix, 2 L2R, 3 R2L, 4 B2T, 5 T2B), 0x2C = time in ms.

> ⚠️ **Embedded images are read-only from the app's point of view.** The app opens and draws embedded images (SafeBMP flag 0) normally. But when it re-saves, it writes the image block with size 0 and no data, and creates no folder. Reopening that file shows "NO Image". **This tool never creates embedded images.**

> **Transition timing looks frame-based [confirmed: measured].** A 2 s L2R transition took about 14 s on a PC rendering about 10 fields/s, producing 135 distinct output fields (≈ 2 s × 60 fields). So on a slow PC that cannot keep up with the output frame rate, transitions stretch proportionally. This is why the inspection report warns about non-CUT transitions.

> **Auto-advance only works while the app's global Auto mode is on [confirmed].** Page header 0x1C/0x20 (auto-advance enabled, interval) alone does not advance pages.

**Open items**
1. Freehand (type 1) never renders from points written into a file (tried i32/f32, relative/absolute). Needs a freehand drawn in the app, saved, and diffed.
2. RichText bold/italic: setting the style byte (b24: 1 bold, 2 italic) turns on the property-panel button, but glyphs are not drawn bold/italic. Re-check on real Windows.
3. Record 0xB0 (shape preset): no effect on rendering or the property panel.
4. The letter-spacing field has not been located.

## 1. Overall structure
```
FileHeader   0x28 B
DocHeader    0x20 B
[ver>=0x54] Layer  (global Background layer)
i32 pageCount
Page × pageCount
```
No compression or chunk table; it is a stream read front to back.

## 2. FileHeader (0x28)
| off | type | meaning |
|---|---|---|
|0x00|char[8]|`HDCGData` [confirmed]|
|0x08|i32|File version. The app always saves 0x78 (120) and refuses to load anything greater than 0x78 [confirmed]|
|0x0C–0x27|—|**Meaningless values.** The app writes uninitialised memory here, so it differs per file (e.g. 0x664430). **Preserve byte for byte** [confirmed]|

## 3. DocHeader (0x20)
| off | type | meaning |
|---|---|---|
|+0|i32|Video format index (5 in samples). If it differs from the app setting, the app asks whether to convert. Treated as 5 when ver<0x28|
|+4..+1F|—|All 0 in samples. Meaning unknown [confirmed: values only]|

## 4. Layer
| item | type | meaning |
|---|---|---|
|hdr|0x18 B (4 B if ver<0x28)|+0 i32 object count. +4 i32 format index (5). +8..+17 are 0 [confirmed]|
|objects|Object × n||

The global Background layer (v10) uses the same Layer/Object format as the Contents layer, e.g. one type 5 image object [confirmed].

## 5. Page
| item | type | meaning |
|---|---|---|
|hdr|0x6C B|see table below|
|i32|layer count|always 1 in samples (Contents layer)|
|Layer × n|||
|cues|40 × hdr[+8] B|Present only if hdr+8 > 0. The app skips it without reading (probably legacy data). Preserve byte for byte|
|thumbnail|BMP|Present if byte hdr+2 is non-zero. Samples: 384×216 32bpp, 331,830 B. **Does not contain the original images**|

Page hdr (0x6C):
| off | type | meaning | evidence |
|---|---|---|---|
|+0..1|—|0|confirmed|
|+2|u8|thumbnail-present flag (0 if ver<0x22)|confirmed|
|+4|i32|format index (5); 5 if ver<0x28|confirmed|
|+8|i32|cue record count (× 40 B)|confirmed|
|+0x0C, +0x10|i32, i32|page resolution 1920, 1080|confirmed|
|+0x1C|i32|auto-advance enabled|unverified|
|+0x20|i32|auto-advance interval (seconds, UI 5–600, default 60 if ver<0x52)|unverified|
|+0x24|i32|**page transition enabled**|confirmed|
|+0x28|i32|**transition type**: 0 Cut, 1 Mix, 2 L2R, 3 R2L, 4 B2T, 5 T2B|confirmed|
|+0x2C|i32|**transition time in ms** (0.5–3.0 s in steps of 500, default 500)|confirmed|
|rest|—|0 in samples|confirmed: values only|

Transition settings are not visible in the editor; they apply only on output. If the enabled flag is 0, the app's global effect is used.

## 6. Object
```
rec      0xF8 B  (fixed record, §6.1)
[ver>=0x76] i32[rec+0xF0 count]
sub      0x20 B
[sub.hasImage] SafeBMP
SafeBMP × sub.nImages            (sequence or image list)
i32 seqInterval, seqLoop, [ver>=0x15] seqDelay, seqLoopCount   (§6.1b)
[sub.nPoints>0] point list n × 12 B (§6.1c)
UTF16[sub.lenText]               plain text (type 7 truncates to 2000 chars)
UTF16[sub.lenText2]              used by type 6; probably a font or template name
UTF16[sub.lenPath] + [ver>=0x6E] u64 stamp   external file path (video, ...)
[sub.hasRich] RichText (§6.4, ver>=0x64)
[ver<0x29 && len4>0] UTF16[len4] skipped
```
sub (0x20): +0 u8 hasImage, +1 u8 hasRich, +4 i32 nPoints, +8 lenText, +0xC lenText2, +0x10 lenPath, +0x14 nImages, +0x18 len4.

### 6.1 Fixed record 0xF8
| rec | type | meaning | evidence |
|---|---|---|---|
|0x00|f32×2|centre x, y (px)|confirmed|
|0x08|f32|**rotation (radians), positive = clockwise** (+0.5236 = 30° clockwise)|confirmed|
|0x0C|f32×4|l, t, r, b relative to the centre|confirmed|
|0x1C|i32|object type: 1 freehand, 2 line, 3 rectangle, 4 ellipse, 5 image, 6 text, 7 video, 8 PIP|2–6 confirmed, 1·7·8 unverified|
|0x20|u32 ARGB|**line (border) colour**, alpha = byte 0x23|confirmed|
|0x24|u32 ARGB|**fill colour**, alpha = byte 0x27. Also the Alpha of images/videos|confirmed|
|0x28|f32|**line width**|unverified|
|0x2C|f32|candidate corner-related value|guess|
|0x30|u32 ARGB|**text colour** (alpha = byte 0x33)|confirmed|
|0x34|f32|**font size**|confirmed|
|0x38|u8|unknown (0 in samples)|—|
|0x39|u8|**word wrap**. Forced to 1 when the scroll type changes|unverified|
|0x3A|u8|legacy scroll type (ver<0x51)|unverified|
|0x3C|i32|**horizontal alignment**: 0 centre, 1 left, 2 right|confirmed|
|0x40|i32|**vertical alignment**: 0 middle, 1 top, 2 bottom|confirmed|
|0x44|u32 ARGB|**outline colour** (alpha = 0x47). Sample ff000000 black|unverified|
|0x48|i32|**outline enabled**. Sample −1 (probably "use parent/per-character value")|unverified|
|0x4C|i32|**outline width** (sample 3)|unverified|
|0x50|i32|**scroll speed** (sample 3)|unverified|
|0x54|i32|**PIP shape** (rectangle/ellipse)|unverified|
|0x58|i32|**gradient enabled**|confirmed|
|0x5C|i32|**gradient direction**: 0 up, 1 right, 2 down, 3 left|confirmed|
|0x60|i32|**gradient alpha/colour**|unverified|
|0x64|i32|**PIP input channel** (−1 none, 1–4)|unverified|
|0x68|i32|**crop enabled**|confirmed|
|0x6C|f32×4|**crop UV rectangle** (default 0, 0, 1, 1)|unverified|
|0x7C|i32|**crop fit**|confirmed|
|0x80|i32|**shadow enabled**|confirmed|
|0x84|i32|**shadow angle** (default 45; 135 -> towards upper right)|confirmed|
|0x88|i32|**shadow distance** (default 20)|unverified|
|0x8C|u32 ARGB|**shadow colour + alpha** (default 0x80000000)|unverified|
|0x90|i32|**skin enabled**|unverified|
|0x94|i32|**skin fit**|unverified|
|0x98|f32×2|**skin repeat x, y** (default 1, 1)|unverified|
|0xA0|i32|unknown|—|
|0xA4|i32|**draw line inside**|unverified|
|0xA8|i32|**line spacing**|confirmed|
|0xAC|i32|**scroll type** (0 none)|confirmed|
|0xB0|i32|shape preset number 0–15. No effect on rendering or the property panel|guess|
|0xB4|i32×4|**corner kinds** TL, TR, BR, BL: 0 square, 1 cut, 2 round|confirmed|
|0xC4|f32|**corner size** (1e9 = fully round)|confirmed|
|0xC8|i32|**scroll line by line**|unverified|
|0xCC|i32|**scroll loop** (sample −1 = on)|unverified|
|0xD0..0xE7|—|unknown (0 in samples)|—|
|0xE8|u8|**item-from-template marker.** Used to select targets when re-applying a template, generating a hymn set, or applying a font to all pages. No effect on rendering|unverified|
|0xE9|u8|**lock**|unverified|
|0xEA|u8|**visible** (0 = hidden)|confirmed|
|0xEC|i32|unknown|—|
|0xF0|i32|length of the following i32 array (ver>=0x76)|confirmed|

**Text note:** font name, bold, italic, and text colour are stored in RichText (§6.4) per-character records, not in the object record. Record fields 0x30/0x34/0x44.. are defaults; a RichText value other than −1 takes precedence.

### 6.1b Object tail i32×4: sequence animation
| order | meaning | evidence |
|---|---|---|
|1|frame interval (ms), UI 15–100|confirmed|
|2|loop (−1 on, 0 off)|unverified|
|3|start delay (stored in ms, shown in seconds, UI 0–60) — ver>=0x15|confirmed|
|4|loop count — ver>=0x15|unverified|

Frame images are the list of sub.nImages SafeBMPs (in file-name order).

### 6.1c Point list
sub+4 is the **point count n**. For ver>=0xF it is `n × {i32 kind, i32 x, i32 y}` (12 B). kind<2 means an (x, y) point; kind>=2 copies the previous point. A line (type 2) is drawn from 2 i32 points [confirmed].

### 6.2 SafeBMP
| off | type | meaning |
|---|---|---|
|0|char[8]|`SafeBMP ` (trailing space) [confirmed]|
|8|i64|payload size (≤0 = empty image) [confirmed]|
|0x10|u32|Non-zero (ffffffff): payload is a **path** (UTF-16LE). **0: payload is the raw image file bytes** (embedded). Treated as 0 when ver<0x21 [confirmed]|
|0x14|u64|Original file modification time (stamp). Low 32 bits: ms since midnight; high 32 bits: days since 0001-01-01 plus 1 (local time). ver>=0x6E [confirmed]|
|0x1C|u32|0 [confirmed]|
|0x20|payload||

The app always seeks to `start + 0x20 + size` to read the next block.

**Image lookup order [confirmed]:** ① `<hcg name>\<ID><file name>` ② the original absolute path ③ fallback paths -> dialog.

### 6.2b Embedded images (flag 0)
The payload is **the original image file bytes as is** (no extra header). PNG, JPG, and BMP all load [confirmed]. As warned above, re-saving in the app drops them, so this tool never creates them.

### 6.3 Image-folder file-name ID [confirmed]
```
s         = IntToHex(stamp, 4) + LowerCaseASCII(original absolute path)   # upper-case hex, no leading zeros (min 4 digits)
ID        = CRC32(UTF-16LE(s), initial value 0x47434448 'HDCG')            # standard reflected polynomial 0xEDB88320
file name = '%.8X' % ID + file-name part of the path
```
In Python: `zlib.crc32(s.encode('utf-16le'), 0x47434448)`. **All** 27 files in the sample image folders match.
- Only ASCII `A`–`Z` are lower-cased.
- If the original file's modification time (stamp) changes, the ID changes.
- **If the path or extension changes (e.g. png -> jpg), the ID must be recomputed.**

### 6.4 RichText (ver>=0x64)
```
WStr text
one record per character (text length):
  u8 b0       (probably a style index)
  u8 b1       (probably outline enabled)
  u8 b2, b3
  u8 b4       (probably shadow enabled)
  u8 b5, b6
  u8[4]       (per-line layout flags, etc.)
  i32 +14     (probably outline width)
  i32 +18     per-character colour ARGB (−1 = default; v06 red = 0xFFFF0000) [confirmed]
  i32 +1C     (probably shadow size)
  i32 +20     (−1 = default)
  u8 b24      style bits: 1 bold, 2 italic (storage confirmed, rendering not)
  WStr font name (empty = object default font)
```

## 7. Versions
The current app saves 0x78, and every test file is 0x78. **This tool supports only 0x78.** Older files (saved by an older app and never re-saved) become 0x78 once opened and saved in the current app, so the report tells the user to do that. Newer versions cannot be supported until samples exist.

For reference, the layout changes at these version boundaries: 0x15 (sequence tail), 0x21 (SafeBMP flag), 0x22 (thumbnail flag), 0x28 (layer hdr size, format defaults), 0x29 (len4), 0x54 (global layer), 0x64 (RichText), 0x6E (stamp), 0x76 (i32 array).

## 8. Mapping of manual features to fields
Image/video position · size · rotation -> 0x00/0x0C/0x08 · Alpha -> 0x27 · sequence -> tail i32×4 · text font/colour/border -> RichText + 0x30/0x34/0x44–0x4C · scroll -> 0xAC/0x50/0xC8/0xCC · skin -> 0x90/0x94/0x98 · shape colour/width -> 0x20/0x24/0x28 · gradient -> 0x58/0x5C/0x60 · shadow -> 0x80–0x8C · PIP -> 0x68–0x7C/0x54/0x64 · lock -> 0xE9 · layers (Overlay/Contents/Background) -> global layer (Background) + page layer · transition effect (CUT/MIX/left-right-up-down, 0.5–3 s) -> page hdr 0x24–0x2C. Template and Bible features have no dedicated fields (templates copy page content).

## 9. Open questions
- Unknown record fields: 0x38, 0xA0, 0xB0, 0xD0–0xE7, 0xEC, and the purpose of the i32 array after 0xF0.
- Exact meaning of RichText character-record fields b0, b2, b3, b5, b6, +0x1C, +0x20.
- Meaning of 0x48 = −1 (probably "use parent value").
- DocHeader after +4, Layer hdr after +8 (all 0 in samples), page cue records (40 B).

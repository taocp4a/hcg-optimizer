# Sample files

All test `.hcg` files live flat in this folder. An image folder sits next to an hcg when it has the same name (`v07_image.hcg` + `v07_image/`), the way the app saves it. Every file is format version 0x78 and was made with ez-CGER 2.10.65, either directly in the app or by changing one to three fields of an app-made file.

The tests use **all** `*.hcg` files here:
- `project/scripts/reference_roundtrip.sh`: the Python reference must read and re-write each file byte-identically.
- `reference_summary.json`: the Python reference's view of each file (pages, objects, image references). The Rust tests compare against it. Regenerate it with `project/scripts/reference_summary.py` after adding or changing a sample.
- `crates/hcg-format/tests/samples.rs`: contract tests (roundtrip, truncation, mutation, edits, folder file names).

`cp949_sample_SIMULATED.zip` is the one zip here: a simulated Windows Explorer zip whose single entry name, `찬양배경/주님의 은혜.jpg`, is stored as CP949 bytes without the UTF-8 flag. The zip adapter tests (`crates/hcg-adapters/tests/archive.rs`) read it. Other test zips are built on the fly from the files here.

To add a sample, drop it here (with its image folder, if any), regenerate `reference_summary.json`, and add a row below.

## Overview

| Group | Files | Purpose |
|---|---|---|
| [Base files](#base-files-v) | `v00`–`v12`, `wo_images` | Made step by step in the app; the base for all property tests |
| [Large images](#large-images) | `layered`, `speedtest` | Realistic multi-page files with large images. Zip one with its folder for a full tool input |
| [Rectangle properties](#rectangle-properties-r-a-c-g) | `r*`, `a01`, `c*`, `g*` | Rotation, colours, line, corners, shadow, gradient, flags |
| [Text properties](#text-properties-t-a02-l-b-k) | `t*`, `a02`, `l_*`, `b*`, `k*` | Alignment, outline, scroll, wrap, skin, line spacing, RichText styles |
| [Image properties](#image-properties-i-q) | `i*`, `q_*` | Alpha, crop, sequences, **embedded images**, empty image block |
| [Lines and points](#lines-and-points-f-s) | `f*`, `s*` | Point lists of freehand (type 1), line (type 2), ellipse (type 4) |
| [Pages](#pages-p) | `p*` | Page transition effects and auto-advance |

Image folders present: `v07_image`, `v08_image_moved`, `v10_bg`, `v11_two_images`, `v12_sameimg_twice`, `wo_images`, `layered`, `speedtest`. Property-test files reference images (e.g. `C:\test\mark_1.png`) but have **no folder here**, so they double as missing-file cases for the report.

## Base files (`v*`)

| File | Content |
|---|---|
| `v00_empty.hcg` | Empty document, one page |
| `v01_2pages.hcg` | Two empty pages |
| `v02_textA.hcg` | One text object "A" |
| `v03_textAB.hcg` | One text object "AB" (base for text property tests) |
| `v04_textAB_moved.hcg` | `v03` with the text box moved |
| `v05_font.hcg` | `v03` with a different font |
| `v06_color.hcg` | `v03` with a per-character colour (red, RichText `+0x18` = `0xFFFF0000`) |
| `v07_image.hcg` | One image object `mark_1.png` (base for image property tests) |
| `v08_image_moved.hcg` | `v07` with the image moved |
| `v09_rect.hcg` | One rectangle (base for rectangle property tests) |
| `v10_bg.hcg` | Background image in the global Background layer |
| `v11_two_images.hcg` | Two image objects, `mark_1.png` and `mark_2.png` |
| `v12_sameimg_twice.hcg` | The same image used by two objects |
| `wo_images.hcg` | Byte-identical copy of `v11_two_images` (made to test opening without the image folder) |

## Large images

| File | Content |
|---|---|
| `layered.hcg` | 6 pages, 30 image references to 16 files in `layered/`: small PNG marks and layers, three ~12 MB JPEGs (`big_0..2`), and a 6000×4000 photo (~23 MB). Pages stack up to 11 objects |
| `speedtest.hcg` | 5 pages: the 6000×4000 photo as a global background and on page 5, a small 1080p JPEG, and a 6 MB noise PNG |

## Rectangle properties (`r*`, `a*`, `c*`, `g*`)

All based on `v09_rect`. Offsets are into the fixed object record (HCG_FORMAT.md §6.1).

| File | Change | Expected |
|---|---|---|
| `r01_rotate30.hcg` | +0x08 rotation 0 → 30.0 | Rotation is in radians, so 30.0 is not 30° (see `a01`) |
| `a01_rect_rot_0p5236.hcg` | +0x08 rotation 0 → 0.5236 | 30° clockwise |
| `r02_line_red_w12.hcg` | +0x20 line colour → red; +0x28 line width → 12 | Red border, width 12 |
| `r03_fill_green_a128.hcg` | +0x24 fill colour → green, alpha 128 | Half-transparent green fill |
| `r04_line_inside.hcg` | red line, width 20; +0xA4 line inside → 1 | Border drawn inside the shape |
| `r05_corners.hcg` | +0xB4 corner kinds (1, 2, 3, 1); +0x2C → 80.0 | Different corners |
| `r06_shadow.hcg` | +0x80 shadow on; +0x84 angle 135; +0x88 depth 60; +0x8C red | Red shadow, 60 px |
| `r07_gradient.hcg` | +0x58 gradient on; +0x5C direction 2; +0x60 alpha 200 | Gradient |
| `r08_locked.hcg` | +0xE9 lock → 1 | Looks the same; cannot be moved |
| `r09_flagEA_0.hcg` | +0xEA → 0 | Hidden (0xEA = visible) |
| `r10_flagE8_1.hcg` | +0xE8 → 1 | No visible change (template marker) |
| `c01_preset1_round40.hcg` | +0xB0 preset 1; corners (2, 2, 2, 2); +0xC4 size 40 | All corners round, radius 40 |
| `c02_preset6_pill.hcg` | preset 6; corners (2, 2, 2, 2); size 1e9 | Pill shape |
| `c03_preset8_cut40.hcg` | preset 8; corners (1, 1, 1, 1); size 40 | All corners cut |
| `c04_preset13_mixed.hcg` | preset 13; corners (0, 1, 0, 1); size 1e9 | TL/BR square, TR/BL cut (shows corner order) |
| `g0_gradient_dir0.hcg` … `g3_gradient_dir3.hcg` | gradient on, direction 0 / 1 / 2 / 3, alpha 255 | Up / right / down / left |

## Text properties (`t*`, `a02`, `l_*`, `b*`, `k*`)

All based on `v03_textAB`.

| File | Change | Expected |
|---|---|---|
| `t01_halign_right.hcg` | +0x3C horizontal align → 2 | Right-aligned |
| `t02_valign_bottom.hcg` | +0x40 vertical align → 2 | Bottom-aligned |
| `l_h0.hcg` | horizontal align → 0; box 1000×600 | Centred (0 = centre) |
| `l_v0.hcg`, `l_v1.hcg`, `l_v2.hcg` | vertical align → 0 / 1 / 2; box 1000×600 | Middle / top / bottom |
| `t03_outline_blue10.hcg` | +0x48 outline on; +0x4C size 10; +0x44 blue | Blue outline (RichText values may take precedence) |
| `t04_font_alpha64.hcg` | +0x33 font alpha → 64 | Half-transparent text |
| `t05_rotate_m15.hcg` | +0x08 rotation → −15.0 | Rotated (radians; see `a02`) |
| `a02_text_rot_m0p2618.hcg` | +0x08 rotation → −0.2618 | 15° counter-clockwise |
| `t06_scroll.hcg` | +0xAC scroll type 1; +0x50 speed 10 | Scrolling text |
| `t07_wrap_off.hcg` | +0x39 wrap → 0 | Word wrap off |
| `t08_skin.hcg` | +0x90 skin on; +0x94 fit; +0x98 repeat (3, 2) | Skin settings shown |
| `t09_twolines.hcg` | text "AB\r\nCD", box height 300 | Two lines |
| `t10_twolines_spacing250.hcg` | `t09` + +0xA8 line spacing 250 | Wider line spacing |
| `t11_rich_b0_A.hcg` | RichText char 0: b0 = 1 | Style index on "A" only |
| `t12_rich_outline_A.hcg` | RichText char 0: b1 = 1, +0x14 = 8 | Outline on "A" only |
| `t13_rich_font_A.hcg` | RichText char 0: font "Courier New" | "A" in Courier New |
| `b1_rich_bold_A.hcg` | RichText char 0: raw byte 7 = 1 | Tests the bold byte hypothesis |
| `b2_rich_italic_A.hcg` | RichText char 0: raw byte 8 = 1 | Tests the italic byte hypothesis |
| `b3_rich_bold_b24_A.hcg` | RichText char 0: raw bytes 7 and 27 = 1 | Bold button on (b24 is the style byte) |
| `k1_bold_AB.hcg` | b24 = 1 on "AB" | Bold (stored; not rendered bold) |
| `k2_bold_A_arial.hcg` | b24 = 1, font "Arial", on "A" | Bold "A" |
| `k3_bold_A_courier.hcg` | b24 = 1, font "Courier New", on "A" | Bold "A" |
| `k4_italic_A_courier.hcg` | b24 = 2, font "Courier New", on "A" | Italic "A" |
| `k5_bolditalic_A_courier.hcg` | b24 = 3, font "Courier New", on "A" | Bold + italic "A" |
| `k6_arialblack_A.hcg` | b24 = 0, font "Arial Black", on "A" | Font name only |

## Image properties (`i*`, `q_*`)

All based on `v07_image`.

| File | Change | Expected |
|---|---|---|
| `i01_image_alpha96.hcg` | +0x27 fill alpha → 96 | Half-transparent image |
| `i02_crop.hcg` | +0x68 crop on; +0x6C UV (0.25, 0.25, 0.75, 0.75) | Centre quarter visible |
| `i03_crop_fit.hcg` | `i02` + +0x7C crop fit | Cropped part fills the box |
| `i04_sequence3.hcg` | 3-image sequence (`mark_1..3`); interval 300, loop on, delay 1000 | Sequence settings; images cycle |
| `q_seq_interval_015.hcg` / `_050` / `_100` | 3-image sequence, interval 15 / 50 / 100 ms, loop on | Interval shown in ms |
| `i05_embedded_png.hcg` | **Embedded** image (SafeBMP flag 0): 2537 B PNG, no path or folder | Displays without a folder |
| `i06_embedded_jpg.hcg` | **Embedded** 11912 B JPEG | Same |
| `i07_embedded_bmp.hcg` | **Embedded** 720054 B BMP | Same |
| `i05_resaved.hcg` | `i05_embedded_png` opened and saved again in the app | **Empty image block** (size 0): the app drops embedded images on save |

## Lines and points (`f*`, `s*`)

All based on `v09_rect`, red line width 8. Points are `{i32 kind, x, y}` (HCG_FORMAT.md §6.1c).

| File | Change | Expected |
|---|---|---|
| `f01_polyline_type4.hcg` | type 3 → 4, 3 points | Polyline test on an ellipse object |
| `s01_freeline_t1_rel.hcg` | type 1, 4 i32 points relative to the centre | Freehand W (does not render) |
| `s02_freeline_t1_abs.hcg` | type 1, 4 i32 points in screen coordinates | Freehand W (does not render) |
| `s03_line_t2_rel.hcg` | type 2, 2 i32 points | Straight line (renders) |
| `s04_freeline_t1_nopts.hcg` | type 1, no points | Empty object, no crash |
| `f1_free_f32_rel.hcg` | type 1, f32 points relative to the centre | Freehand hypothesis A |
| `f2_free_f32_abs.hcg` | type 1, f32 points in screen coordinates | Hypothesis A, absolute |
| `f3_free_f32_unit.hcg` | type 1, f32 points in 0..1 | Hypothesis B, normalised |
| `f4_free_f32_rectlocal.hcg` | type 1, f32 points relative to the box's top-left | Hypothesis C |
| `f5_free_i32_allkind1.hcg` | type 1, i32 points, all kind 1 | Hypothesis D |
| `f6_line_t2_f32.hcg` | type 2, f32 points | Whether type 2 accepts floats |

None of the freehand variants rendered (HCG_FORMAT.md, open items).

## Pages (`p*`)

Based on `v01_2pages`; the `p1`–`p5` files also put a red rectangle on page 1 and a blue one on page 2.

| File | Change | Expected |
|---|---|---|
| `p01_page2_effect_L2R_2s.hcg` | page 2 header +0x24 effect on, +0x28 L2R, +0x2C 2000 ms | Left-to-right wipe, 2 s |
| `p02_page1_automode_7.hcg` | page 1 header +0x1C auto-advance on, +0x20 interval 7 | Advances after 7 s (only in the app's Auto mode) |
| `p1_cut_ref.hcg` | no effect | Reference: instant cut |
| `p2_L2R_2s.hcg` | page 2: L2R, 2000 ms | Wipe, 2 s |
| `p3_MIX_2s.hcg` | page 2: Mix, 2000 ms | Dissolve, 2 s |
| `p4_auto_5s.hcg` | page 1: auto-advance 5 s | Advances after 5 s (Auto mode) |
| `p5_auto_5s_L2R.hcg` | `p4` + `p2` | Auto-advance, then a 2 s wipe |

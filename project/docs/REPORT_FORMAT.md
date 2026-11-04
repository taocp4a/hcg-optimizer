# report.json format (schema 1)

Every bundle in the output zip has `<stem>_optimized/report.json`, written by `hcg_app::report` (issue #11). The tool does not render it; **a viewer does** (the web page). For a quick display, use `summary.headline` (one sentence) or `text` (the whole report as plain text, built from the same fields).

Conventions:
- Field names and enum values are `snake_case`. New fields may be added within schema 1; removing or renaming one bumps `schema`.
- Sizes are integers in bytes and pixels; the viewer formats units. Human text (`headline`, `message`, `text`) uses binary units (1 MB = 1,048,576 bytes).
- Positions are **1-based** as users see them: `page`, `layer`, `object`, `frame`. `page: null` means the background (global) layer.
- English only for now. Every warning has a stable `code`, so a viewer can translate by code and fall back to `message`.

Complete examples: `crates/hcg-app/tests/snapshots/every_kind.json` (every item kind) and `failed.json`.

## Top level

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | `1` |
| `tool` | object | `name` (`"hcg-optimizer"`), `version` |
| `bundle` | object | `stem`, `hcg_path` (where the hcg was in the input zip), `folder_path` (image folder in the input, or `null`) |
| `status` | string | `ok` or `failed` |
| `summary` | object | see below |
| `failure` | object or null | `code`, `message`, `advice`, when `status` is `failed` |
| `hcg` | object or null | `version`, `pages`, `image_refs`; `null` if the hcg could not be read |
| `policy` | object | limits used: `max_width`, `max_height`, `max_bytes`, `jpeg_quality`, `min_jpeg_quality` |
| `images` | array | one entry per referenced image-folder file |
| `missing` | array | references whose file is not in the image folder |
| `embedded` | array | embedded images and empty image blocks (never changed) |
| `transitions` | array | pages with their own transition other than CUT |
| `unreferenced_files` | integer | image-folder files no image uses (copied unchanged) |
| `other_files` | object | `unpaired` (zip files that belong to no hcg, not included in the output), `ignored` (count of OS clutter such as `Thumbs.db`) |
| `warnings` | array | everything worth the user's attention, flat |
| `text` | string | the whole report as plain text, lines separated by `\n` |

```json
"summary": {
  "headline": "2 of 3 images shrunk: 28.2 MB -> 2.5 MB. 1 warning.",
  "images_total": 3,
  "images_shrunk": 2,
  "images_renamed": 1,
  "bytes_before": 29554626,
  "bytes_after": 2646745,
  "warnings": 1
}
```

`bytes_before` / `bytes_after` cover the referenced image-folder files.

## `images[]`

| Field | Type | Meaning |
|---|---|---|
| `file` | string | file name in the input image folder (`ID + name`) |
| `path` | string | image path stored in the hcg (e.g. `C:\test\bg.png`) |
| `used_on` | array | where it is used: `page` (null = background), `layer`, `object`, `role` (`primary` or `sequence`), `frame` (1-based position in a sequence, else null) |
| `before` | object | `format`, `width`, `height` (null if unknown), `bytes`, `quality` (null) |
| `action` | string | `shrunk` or `kept` |
| `reason` | string or null | why it was kept (below); null when shrunk |
| `after` | object or null | result when shrunk: `format`, `width`, `height`, `bytes`, `quality` (JPEG quality, else null) |
| `renamed_to` | object or null | when the extension changed: new folder `file` and new hcg `path` |

`format`: `jpeg`, `png`, `bmp`, `gif`, `dds`, `tga`, `webp`, `heic`, `other`.

`reason`:

| Value | Meaning |
|---|---|
| `within_limits` | already within the size and resolution limits |
| `not_resized` | GIF, DDS or TGA: ez-CGER loads it, the tool leaves it as is |
| `not_supported_by_app` | WebP, HEIC or unknown: ez-CGER cannot load it |
| `undecodable` | could not be read as an image |
| `too_many_pixels` | over the decode limit |
| `encode_failed` | could not be saved |
| `no_gain` | re-saving would not save at least 10% |

```json
{
  "file": "2D00575Abg_1080_noise.png",
  "path": "C:\\test\\bg_1080_noise.png",
  "used_on": [{ "page": 4, "layer": 1, "object": 1, "role": "primary", "frame": null }],
  "before": { "format": "png", "width": 1920, "height": 1080, "bytes": 6230528, "quality": null },
  "action": "shrunk",
  "reason": null,
  "after": { "format": "jpeg", "width": 1920, "height": 1080, "bytes": 1754694, "quality": 50 },
  "renamed_to": { "file": "3F8B560Bbg_1080_noise.jpg", "path": "C:\\test\\bg_1080_noise.jpg" }
}
```

## `missing[]`, `embedded[]`, `transitions[]`

| Array | Fields |
|---|---|
| `missing` | `path` (as stored in the hcg), `expected_file` (folder file name the app looks for), `used_on` |
| `embedded` | `kind` (`embedded`: image bytes inside the hcg; `empty`: no image data, e.g. after the app re-saved an embedded image), `bytes`, `used_on` (one place) |
| `transitions` | `page`, `kind` (`mix`, `left_to_right`, `right_to_left`, `bottom_to_top`, `top_to_bottom`, `unknown`), `type_value` (raw header value), `duration_ms` |

## `warnings[]`

Each warning: `code`, `message` (English sentence), `file` (folder file or path concerned, or null).

| `code` | When |
|---|---|
| `missing_image` | a referenced file is not in the image folder (ez-CGER can show it only where the original path exists) |
| `not_supported_by_app` | a referenced file is WebP, HEIC or unknown |
| `over_size_limit` | still over the size limit after shrinking, or kept because re-saving would not help |
| `undecodable` | a referenced file could not be read |
| `too_many_pixels` | a referenced file is too large to process |
| `encode_failed` | a shrunk image could not be saved |
| `transition_not_cut` | a page uses a transition other than CUT (slow PCs stretch transitions) |
| `unsupported_version` | the hcg is not format 0x78 (advice: re-save in ez-CGER) |
| `parse_failed` | the hcg could not be read |
| `apply_failed` | the rewritten hcg failed verification; nothing was changed |

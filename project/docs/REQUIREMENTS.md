# Requirements: hcg image resize and inspection tool

## Goal
Shrink large images inside `.hcg` files of the ez-CGER (LUMANTEK) subtitle program to reduce page-transition lag. Users run Windows PCs at churches and must be able to use the tool without installing anything.

## Form
- Written in Rust. The core logic is shared by WASM (static GitHub Pages site, for users) and a CLI (development and testing). The structure follows project/docs/ARCHITECTURE.md.
- The web page processes everything inside the browser with no server. Files never leave the machine.

## Input and output
- Input: one zip containing an hcg file and an image folder with the same name as the hcg (e.g. `layered.hcg` + `layered/`).
- Entry names inside the zip may be UTF-8 or CP949 (the Windows default for compressed folders); both must be read.
- Output: one zip with the fixed hcg, the shrunk image folder, and an inspection report (report.txt or html).
- Never overwrite the original.

## Processing rules
- Shrink images to fit within 1920×1080 keeping the aspect ratio. Leave images that are already small.
- Target result size ≤ 1 MB. Images without transparency become JPEG (lower the quality until ≤ 1 MB); PNGs that actually use transparency stay PNG.
- The 8 leading hex digits of each image-folder file name are an ID. See HCG_FORMAT.md and `image_id()` in reference/hcg_parse.py. If a path or extension changes, the ID must be recomputed.
- In the hcg, change only image paths and their related length fields; keep every other byte as is.
- Do not use embedded images (flag 0). The app drops them when it re-saves the file.

## Inspection report contents
- Images per page with original size, resolution, and the shrink result
- Files missing from the image folder
- Pages whose transition is not CUT (transitions can take much longer on slow PCs)
- Warnings for unreadable blocks or a different format version

## Acceptance criteria
- Every hcg under samples/ must be byte-identical after being read and written back. The Python reference (reference/hcg_write.py) passed this on 90 test files, excluding files with embedded images.
- When the resized hcg is read back, only image paths may differ; all other fields must be equal.
- The parser must raise an error instead of silently skipping bytes it could not handle.

## References
- HCG_FORMAT.md: format spec. The evidence column marks each item [confirmed], [unverified], or [guess].
- reference/hcg_parse.py, reference/hcg_write.py: verified Python implementation; the baseline for the Rust port.
- samples/fmt: step-by-step files made directly in the app (from v00 empty file to v12 same image twice), with image folders.
- samples/rev: a few property test files.
- Large-image tests use samples/large (speedtest.hcg + speedtest/ folder, including a ~10 MB 6000×4000 photo and a 6 MB PNG). Zipped, it is also an example input for the tool.
- The vendor's program, DLLs, and manual are not included and not redistributed.

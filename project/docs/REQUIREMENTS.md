# Requirements: hcg image resize and inspection tool

## Goal
Shrink large images inside `.hcg` files of the ez-CGER (LUMANTEK) subtitle program to reduce page-transition lag. Users run Windows PCs at churches and must be able to use the tool without installing anything.

## Form
- Written in Rust. The core logic is shared by WASM (static GitHub Pages site, for users) and a CLI (development and testing). The structure follows project/docs/ARCHITECTURE.md.
- The web page processes everything inside the browser with no server. Files never leave the machine.

## Input and output
- Input: one zip containing an hcg file and an image folder with the same name as the hcg (e.g. `layered.hcg` + `layered/`).
- Entry names inside the zip may be UTF-8 or CP949 (the Windows default for compressed folders); both must be read.
- Output: one zip with the fixed hcg, the shrunk image folder, and an inspection report (`report.txt`). The report is built as structured data (JSON for the web viewer) and rendered to text for the zip.
- Never overwrite the original.

## Processing rules
- Shrink images to fit within 1920×1080 keeping the aspect ratio. Leave images that are already within the size and resolution limits.
- Target result size ≤ 1 MiB by default (user-adjustable). Lossy compression is acceptable: images without transparency become JPEG, starting at quality 90 (user-adjustable) and lowering it until the size limit is met. Only PNGs that actually use transparency stay PNG; preserving alpha is what matters.
- Keep the EXIF orientation of the source.
- The 8 leading hex digits of each image-folder file name are an ID. See HCG_FORMAT.md and `image_id()` in reference/hcg_parse.py. If a path or extension changes, the ID must be recomputed.
- In the hcg, change only image paths and their related length fields; keep every other byte as is.
- Do not use embedded images (flag 0). The app drops them when it re-saves the file.

## Image formats
ez-CGER's image-open dialog accepts exactly six formats: PNG, JPG/JPEG, BMP, TGA, DDS, GIF. WebP is not among them (the program mentions "webp" only in its video decoder's format list).

- Recognise formats by their leading signature bytes, not the extension. TGA has no signature, so it is recognised by its extension.
- **Shrink:** PNG, JPG, BMP (result JPEG; PNG only when transparency is used).
- **Leave as is, show the size in the report:** GIF (may be animated; the app ships a GIF frame extractor), DDS (GPU texture format), TGA (could be shrunk, but rare).
- **Other formats (WebP, HEIC, ...):** if the hcg references one, warn that ez-CGER does not support the format; if no hcg references it, ignore it.

## Inspection report contents
- Images per page with original size, resolution, and the shrink result
- Files missing from the image folder
- Pages whose transition is not CUT (transitions can take much longer on slow PCs)
- Warnings for unreadable blocks or a different format version

## Page overview PDF (issue #19)
- From the same input (or a single `.hcg` without its image folder), make a PDF that shows every page as the thumbnail the app stored in the hcg, several per sheet, so the service flow can be checked and shared without ez-CGER. Available in the CLI (`--pdf`) and on the web page.
- A4 landscape; 6, 12 (default) or 20 thumbnails per sheet; page numbers under each thumbnail; blank pages kept so the numbering matches the app.
- Visible text is ASCII only (no embedded fonts); the hcg name goes into the PDF title and the file name (`<hcg name>_pages.pdf`).
- A separate download, not part of the optimized zip.

## Acceptance criteria
- Every hcg under samples/ must be byte-identical after being read and written back. The Python reference (reference/hcg_write.py) passes this on all 91 test files, including embedded images and an empty image block.
- When the resized hcg is read back, only image paths may differ; all other fields must be equal.
- The parser must raise an error instead of silently skipping bytes it could not handle.

## References
- HCG_FORMAT.md: format spec. The evidence column marks each item [confirmed], [unverified], or [guess].
- reference/hcg_parse.py, reference/hcg_write.py: verified Python implementation; the baseline for the Rust port.
- project/samples: all 91 test files in one folder, described in project/samples/README.md: step-by-step base files (v00 empty file to v12 same image twice), property test files, embedded-image files, and two realistic files with large images.
- Large-image tests use `layered.hcg` + `layered/` (three ~12 MB JPEGs, a 6000×4000 photo) and `speedtest.hcg` + `speedtest/` (the photo, a 6 MB PNG). Zipped with its folder, each is also an example input for the tool.
- The vendor's program, DLLs, and manual are not included and not redistributed.

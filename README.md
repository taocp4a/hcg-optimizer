# hcg-optimizer

A tool that shrinks large images inside ez-CGER `.hcg` files to reduce page-transition lag.
It is written in Rust, compiled to WASM, and served from GitHub Pages. Files never leave the browser. A CLI for development and testing uses the same core.

- Usage: zip a `.hcg` file together with its same-named image folder and drop the zip in. You get back a zip with shrunk images and an inspection report.
- Page overview: drop a zip or a single `.hcg` and download a PDF with every page's thumbnail, 6, 12 or 20 per A4 sheet.
- Developer and agent guide: [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md)
- Requirements: [`project/docs/REQUIREMENTS.md`](project/docs/REQUIREMENTS.md)
- Format spec: [`project/docs/HCG_FORMAT.md`](project/docs/HCG_FORMAT.md)

```bash
project/scripts/preflight.sh   # required checks before a PR
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

ez-CGER is a product of LUMANTEK. This project is not affiliated with LUMANTEK and does not include or redistribute the vendor's program or manual.

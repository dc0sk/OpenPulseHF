# JS8 JSC codebook (`jsc_codebook.bin.z`)

The 262 144-entry word/substring dictionary JS8Call's `JSC` text coder uses, embedded for
[`crate::jsc`]'s `jsc_decompress` (general JS8 free-text decode). Ported verbatim from GPL-3.0
JS8Call `jsc_map.cpp` (license-compatible with this repo).

## Format

zlib-compressed, decompressing to `[count:u32 LE][ (len:u8)(latin1 bytes) × count ]` — one entry per
codebook index, in the same order as `JSC::map[]`. Loaded and expanded once on first use.

## Regeneration

Extracted from the upstream `jsc_map.cpp` (each line `{"<c-escaped string>" /* opt comment */, <len>,
<index>}`). The parser must handle the C escapes (`\\`, `\n`, `\xNN`, `\"`) and the trailing comments —
missing any entry shifts every higher index and silently corrupts high-index decodes. Validated
against Qt5-compiled `Varicode::unpackDataMessage` ground-truth vectors in `src/jsc.rs`.

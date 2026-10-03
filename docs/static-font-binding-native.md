# Native static-font binding acceptance

The Linux X11 fixture completed **2026-10-03 00:44:56–00:47:51 UTC**, with normal
exit **0**, **28 physical actions across 11 buttons**, and **21/21 trace checks**.
Independent provenance checks passed **17 general checks and 34 font checks**.
This accepts the exercised startup registration and binding-level candidate
contract. Canvas strokeText placement, geometry and publication are separate work.

Implementation/fixture source: [`452ebe67`](https://github.com/emojiiii/gpui-flow/commit/452ebe679e3f68becc9d9b97976d32c55037b782).
The machine-readable summary (historical record in Git history)
records the frozen source, executable, fonts, report checksums and scope.

## Actual native observations

- The two same-App prepared faces resolved to App FontIds **0 (TTF)** and
  **1 (CFF)**. Accepted actual runs used those IDs, immutable startup PreparedFont
  objects and valid glyph IDs 1–4. FontId 0 is valid; missing glyph ID 0 is different
- Native WrappedLine objects rendered the geometric fixture text at 28px/38px,
  width 340, with two wrapped rows. Initial displayed objects are the same
  initial objects inspected by the fixture
- `WQ` used actual unbound FontId **5**, with nonzero glyph IDs 58/52, and was
  rejected. Mixed text produced **[0,5,0]**; all runs were collected before the
  complete binding candidate was rejected
- Reusing a real numerical ID with a separate Noop TextSystem was rejected.
  Weight 700 and italic requests were rejected against the regular-400 faces
- Cancellation and old epochs rejected; fresh requests recovered. Epochs advanced
  0→1→2→3→4→5; repeated add-font controls did not register again
- Adding original unrelated bytes produced unbound ID **6**. App IDs and fresh
  shaped line structures for both bound faces remained unchanged after this
  addition and after appending identical prepared bytes under the same alias

The audit recomputed 36 candidate decisions, 42 run observations and 720 glyph
observations; these are repeated observations, not 720 distinct glyphs. It checked
finite positions, valid UTF-8 indices, metrics and wrapping. All 29 stdout JSON
records equal their saved session records, and stderr is empty.

Direct-native IDs remain private: their equality with App IDs is checked inside
the factory, not independently queried by this fixture. Later fresh shaped lines
were compared in receipts while panels continued painting the initial lines.
The same-alias case covers identical-byte duplication only, not hostile different
font bytes. Wrong-owner testing did not construct a second native application.
Ambient fallback font bytes were neither extracted nor redistributed.

## Preserved evidence and limits

The final 1000×800 layout showed all 11 PASS statuses and the complete footer.
The **12 original JPEG captures** total 1,783,468 bytes and preserve visible
samples during expected rejections. Their lossy pixels do not prove outline,
antialiasing, framebuffer or Canvas pixel identity.

The saved session contains both actual source buffers and renamed registration
buffers. TTF source/prepared sizes are 1540/2048 bytes; CFF sizes are 1576/2084.
Hashes match the saved bytes. Table and whole-font checksums validate, and only
the name table and head checksum adjustment changed. The CFF internal FontName
remains unchanged while its SFNT names carry the private alias. Registration
hashes are specific to this launch's random aliases. All font data is from the
first-party CC0 fixtures.

The actual link has **17 contemporaneous before/after input hashes**: 16 source
archive files plus the separately preserved contract. The **476-file** archive
matches hashes around a later Cargo freshness check that reused the unchanged
binary. Those 476 hashes do not expand the actual-link proof. They match the
published source tree; external dependencies, compiler and system libraries are
excluded, with no full-rebuild or reproducible-build claim.

This run does not validate Wayland/other platforms, positive bold/italic faces,
new stroke-test fonts or their GSUB/GPOS features, outline placement/stroking,
affine/DPR/clip/alpha behavior, resource budgets or atomic Canvas publication.
ELM-10 remains partial. The earlier model and compile checks remain separate from
this native result; no native window was reopened during the subsequent audits.

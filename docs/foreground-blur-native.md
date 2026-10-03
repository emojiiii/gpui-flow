# Foreground blur: bounded native acceptance

The 12-row physical matrix completed on the dot cloud Linux desktop at actual display DPR 1. The expected visible behavior, public diagnostics, history, navigation and pointer routing were observed. The process closed normally once at **2026-10-03 11:15:10.948935 UTC**, exit **0**, with empty stderr. No unexpected diagnostic or persistent failure was observed in this run. Ordinary pending transitions and the two intended `UnsupportedBlurBackdrop` states are retained.

The final independent read/parse/document/hash audit by the same QA operator passed **210/210** checks. This is an evidence audit, not a second execution or separately staffed renderer review. The raw material comprises **79 physical clicks, 82 numbered JSON events, 34 settled observations, 7 actual PointerDown traces, and 46 original JPEG screenshots**. The evidence was captured through supported cloud desktop controls; no event injection or helper was substituted for physical input.

## Frozen identity and actual window

- Source checkpoint: `723d923dff24063ae4bc07a56917965674dd4c2b`, tree `9d9bfb467bc13361d4a0115b156a9c5ef0ff58f8`
- Executable: `foreground_blur`, 180,366,392 bytes, SHA-256 `164a5130ff32d177f49a1ac382ff175fffa13bb1046c57feb7c69a89d3ce9beb`; actual bytes agree with before/after native receipt
- Archive: 1,911,287 bytes, SHA-256 `5dcbbd56fb5d16302a3aebc5196e73d47d1bdfaf3e233f4e5b391f403162e1d4`
- All 696 archived repository entries independently matched recorded bytes, SHA-256 and Git blob identity. The 529-file conservative code/assets/Cargo inventory matched before/after linking and the archive. All 331 initial freeze checks remain intact. This inventory is not a compiler dependency graph or a fully reproducible build claim
- Session: `session-data/foreground-blur-physical-75490-1791025542630857867`
- Run: 11:05:42.590266–11:15:10.948935 UTC, host `d2b65af346d2`
- Desktop: 1364×1024. The window first appeared at a larger restored host size; a normal titlebar restore produced a client rectangle `(92,332)–(1152,932)`, 1060×600, and canvas `(100,440)–(540,660)`, 440×220. All three control rows and footer lines fit, including wrapped B/D/E hints. Capture `main-03` records this layout
- The actual public window DPR was 1 in all 34 observations. Zoom 1 and 2 were physically exercised. Synthetic helper DPR 2 does not count as another physical display configuration

## Physical rows

Event numbers below refer to the single physical session's `NNN-event.json`. Screenshot numbers refer to original `screenshots/main-NN-*.jpg`. `physical-row-records.json` carries exact paths, capture times and hashes.

| Row | Actual observations and evidence | Verdict and boundary |
| --- | --- | --- |
| 01 | At zoom 1, sharp red reference, blurred red middle and dark gray half-opacity complete result were visible. Interior world `(65.0011,36.0022)` routed to `color`. Event 001 actual input; 002 settled baseline. Shots 03–04 | Expected baseline and footer fit observed |
| 02 | Zoom 2 made the red fringe and gray shadow-bearing result easier to inspect. World `(59.0010,36.0014)` had strict, tolerant and routed lists all empty, both blur 1 and 0 (004/007). Zero sharpened the result; restore 1 restored softness. 005/008/010 settled; shots 05–08 | Blur did not expand these strict geometric hits. The adapter's documented four-screen-pixel tolerance is separate; this actual point still missed. JPEG fringes are not an alpha oracle |
| 03 | Blur .5, Undo, Redo and the same history after Invalid model were observed. Eight hidden/zero-opacity negative/above-256/NaN/infinity attempts rejected with the exact model validation error and atomic flags. 012–022; shots 09–14 | Exact authored documents and fingerprints restored. Invalid attempts left revision 5 unchanged; history remained usable |
| 04 | Variant brightened only the gray right-hand result by changing focus opacity .5→1; Undo restored .5. Middle and reference remained visually consistent. Home 1 then Zoom 2 preserved authored source and revision 9. 024/026/028/030; shots 15–18 | Expected visible opacity and navigation behavior observed; no numeric screenshot compositing claim |
| 05 | Fresh A completed four callbacks with .5/0/.5/1 and settled at default source (032–036). A second burst emitted .5/0/.5 (037–039); prompt B replacement logged one remaining retired callback (040), a distinct entity and fresh revision 0 (041). Shots 19–21 | This invocation actually retired a nonzero remaining count. No later burst edit appears after B replacement. Exact private upload generation/fence behavior is not exposed |
| 06 | B zoom 2 showed a very faint red strip at the viewport left edge, a red entering child-halo strip around screen x260 and a green projection around x272, y526. Source shows Frame fill/stroke absent and no substitute guide. 043; shots 22–23 | Bounded qualitative visibility observed. These tiny, low-alpha JPEG strips do not prove numeric alpha equality; separate CPU raw/crop checks establish their own exact result |
| 07 | Variant moved root edge source inside and made its red body visible; Undo restored outside. Blur 0 removed entering coverage and the derived green projection; Undo restored all faint strips. 045/047/049/051; shots 24–28 | Authored changes are exactly root x or the two intended blur scalars. All settled checkpoints have no issues |
| 08 | C zoom 2 showed red child, blue parent, magenta stroke and green shadow within the rounded inherited region. Child interior routed `child`; full candidate list included `parent`. Frame→Box visibly expanded child overflow, Undo restored Frame. 054/055/057/059; shots 29–32 | Exact source change is shape type plus `clip_children:true→false`; style/children unchanged. Selection overlay remains distinguishable from paint. Exact crop bytes belong to separate CPU audit |
| 09 | D showed a soft masked red strip, softened Pixel removal, and hard transparent-looking Path cut; later green survived in Pixel/Path columns. Actual clicks routed `pixel` and `path` even where corresponding paint is removed. Path-only blur 1→0 and Undo kept the visible cut consistent. 062–069; shots 33–37 | Correct geometric picking is separate from painted coverage. Selection outlines are not counted as surviving scene paint; JPEGs are not used for bit-exact comparison |
| 10 | Blur 0 sharpened mask/Pixel boundaries while Path remained geometric. Undo restored the positive-blur appearance. 071/073; shots 38–39 | Full strict-probe signatures and world bounds stayed equal to the positive-blur baseline |
| 11 | Fresh E rejected the valid authored PassThrough+Multiply composition with `UnsupportedBlurBackdrop(DisplayId("focus"))`. The OwnedCanvas error display was captured; pending=false and geometry available=true were recorded. 075; shot 40 | Expected rejection. Geometry availability is not rendering acceptance and no unfiltered substitute is treated as success |
| 12 | Explicit Normal produced a visible blue child over earlier red with later magenta preserved. Actual child click routed `multiply`, candidates also contained `external`. Undo restored the typed rejection; Redo restored the Normal result with no settled issues. 077/078/080/082; shots 41–45 | Expected recovery/history behavior observed; one normal close yielded external exit receipt 0 |

## Trace and artifact audit

The one plain stdout startup line is preserved and checked exactly. The following 82 JSON lines match all 82 numbered saved event files structurally, with no missing or extra event. The session also retains its manifest, source identity and exact compiled fixture source: **85 files, 746,158 bytes**. The compiled source bytes equal the frozen fixture; its independently calculated FNV-1a agrees with the runtime identity.

Event kinds are 34 native observations, 17 host edits, 12 history records, 7 actual input traces, 6 navigation records, 5 preset replacements and one invalid-model record containing eight attempts. The 17 host edits each report no error, readonly host access, a changed source and one incremented revision. Every intended settled observation has pending=false; the only settled composite issues are the exact two required E rejections. All observed settled render issues are empty. Public geometry is available even for the rejected E source, demonstrating why it is not an upload proof.

Complete strict/tolerant candidate lists were checked, not confused with the single routed top target: `color`; two empty halo lists; `child,parent`; `pixel,pixel-parent-red`; `path,path-parent-red`; `multiply,external`. Actual traced world coordinates retain the desktop input's small subpixel offsets. Viewport conversion and four-screen-pixel tolerance values agree with the saved trace.

Every original capture has JPEG/JFIF magic `ffd8ffe000104a4649460001`, MIME `image/jpeg`, true `.jpg` extension, intact SHA/byte count and decoded dimensions 1364×1024. Total original screenshot bytes: **6,803,472**. A fresh literal 1049Z capture closure and exclusive creation were used; the first destination/header were printed before launch. No old package was overwritten, and no JPEG was re-encoded as purported raw PNG.

## Preserved intermediate states and audit corrections

- `main-01` is the desktop before the process appeared, not a rendered fixture. `main-19` is an immediate burst capture, not evidence that every callback had already been presented
- `main-22` shows normal Compositing/pending during B zoom; `main-23` shows settled B. `main-24` shows moved content with pending=true, followed by settled `main-25`. `main-33` shows D content while pending=true, followed by settled `main-34`. These originals remain unchanged
- E immediate history event 079 has the new unsafe source before the later rejection diagnostic; 080 records the settled rejection. Immediate Redo event 081 still carries the previous rejection, while 082 records settled Normal without issues. Source state and immediate diagnostics must not be conflated
- Audit development first stopped on the plain stdout startup line, then on an incorrect auditor assumption about the Shadow JSON field name. Both script versions and a correction record are retained; no native file changed
- The first complete audit remains **209/210** in `native-audit-draft-209-of-210.json`. Its sole failed assertion incorrectly required Frame→Box to change only `shape.type`. Inspection showed the expected `shape.geometry.clip_children` true→false change as well. The final assertion explicitly verifies exactly those two fields and all unchanged fields; the independently saved final result is **210/210**. The original failed draft was not erased or relabeled

## Scope and preservation

This fixture physically covers only its small shape presets on this cloud Linux desktop. It does not physically cover Image, Canvas or native Text producers, another OS/DPR, parameter extremes, performance, whole-process/GPU memory, the known legacy A8 tiny-shadow issue, or a browser/Leafer correspondence. There was no reopen. `presented_geometry()` may use a canonical fast path; matching effects, available=true and pending=false do not prove CPU upload generation, GPU fence completion or framebuffer contents.

The frozen helpers' 20 raster combinations, eight independent convolution cases, three crop comparisons, six real PNGs and 264 artifact checks remain separate prevalidation evidence. They were not rerun here and are not native framebuffer readback. The original initial strict failure and original helper batch lacking immediate executable-hash binding remain preserved alongside the later bound frozen replay.

All evidence before the final report additions occupied 21,853,868 bytes excluding the executable, below the 30 MiB target. `seal-receipt.json` records final size and free space. Native execution's minimum free space was 23,759,560,704 bytes; it did not reach the reserve stop threshold.

Pre-native and three row-boundary pauses were backed up to a new, distinct raw-evidence Library identity, `libfile_ec899abdc96481918904276134d65122`. Versions 0–3 preserve these stable stages; v4 preserves the normal-exit raw evidence. Final artifact completeness is confirmed separately after this local seal. That backup status does not replace the bounded QA verdict or strengthen the pixel/generation limitations above.


## Integrator preservation receipt

The sealed operator report above is reproduced unchanged, SHA-256 `e578aa147d8b5d006c93c2a8edb738367c58ba9f83948d7259f0184617061229`. The integrator separately verified all 331 freeze, 46 screenshot, 85 session and 25 native-evidence checksum entries, plus the final ZIP SHA and CRC. The original package was not edited.

The private Library identity `libfile_ec899abdc96481918904276134d65122` is **COMPLETE version5**, filename `foreground-blur-native-qa-20261003T1049Z.zip`, 7,803,309 bytes, SHA-256 `4f51e98f154ccb21f28750420e62a54892b12e2a528a9c836af49579ec432f6b`. Versions0–3 preserve pre-native and row groups, v4 preserves raw normal-exit data, and v5 contains the final seal. All 490 extracted entries passed the preservation coordinator's checksum audit. Only the 180,366,392-byte rebuildable executable is excluded; 488 original files, source/rebuild identity, initial failures, both helper batches, every original JPEG/event and every audit draft remain. Total original nonbinary evidence is22,063,983 bytes. COMPLETE describes artifact collection, not broader native/GPU or cross-platform coverage.

This completes only the foreground-blur increment's 12-row bounded physical gate. APP-04 remains partial: the original shadow-mode and owned-background-blur requirements remain. The known legacy A8 tiny-shadow issue is not hidden or claimed fixed by this gate. No Actions, deployment, new native run or broader test suite was started for this documentation checkpoint.

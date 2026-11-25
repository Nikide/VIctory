# Steam Deck, 25 November 2026: the world is shown, at 1 frame a second, without textures

Measured on the Deck (SteamOS, RADV, Proton `experimental-11.0-20261112`,
`cargo build --profile diag -p victory --features diagnostics`, `--console series-s`),
this tree at `bc58b19`. Numbers only; no game data here.

## What runs

Boot, first-boot screens, menus (30 fps), the opening cutscene, the loading
screen, and the world. The world is **shown** only through
`VICTORY_SHOW_TARGET=1280x720-f28-1` (see below): the title's own final
target, 3840x2160, stays zero, and presenting it is a black screen — the same
shape as upstream #841 ("the prologue renders into the title's 2560x1440
targets but the 3840x2160 output stays zero").

Word frame: **one frame every 5 to 11 s**; the "usual frame of the last 64"
is 2,989 to 3,433 ms, of which **4.7 to 4.8 s per frame is inside submit
calls** (frame log, `framelog: long frame`). That is the wineserver walking
every mapped view on each map and unmap (fxceless #184 §1, #118: the same
frame is 624 ms with a tree-indexed wineserver, 5,398 ms with the stock one).

Textures are missing in the world (surfaces carry no detail); the levers the
trackers name are `VICTORY_TEXTURE_BUDGET_MB` (fxceless #176: 8,000 uploaded
28,533 of the textures the shaders pick by index against 21,050 by default),
the 90 % card-budget rule in `resource::room`, and the host view heap capped
at 1,000,000 against the title's 1,245,184 (upstream #1620, #747).

## How it is run

`~/gta6/env-perf.sh` holds the switches, each with its source in a comment:
no `VICTORY_LATE_WAIT` / `VICTORY_WIDE_BACK` / `VICTORY_RELAY=2` (RADV GPUVM
faults at the end of the loading screen, #184 §3), `VICTORY_SKIP_PIPELINES=
fc5e66f8f873d3bb`, `VICTORY_SHADER_LOOP_LIMIT=4096` (#176),
`VICTORY_WAVE64_EMULATE=wave32` (#161), `VICTORY_COMPOSE_FALLBACK=1`,
`VICTORY_NO_WATCHDOG=1`.

`~/gta6/run_victory.sh` runs the title in a scope of its own
(`MemoryHigh=9G MemoryMax=11G MemorySwapMax=12G`, `oom_score_adj=900`): an OOM
kill lands inside the scope instead of taking the terminal (and the session in
it) down — before this, systemd's `OOMPolicy=stop` on the Konsole app service
did exactly that.

`~/gta6/warm-shaders.sh` fills the shader cache before a run: 25,889
containers found in `shaders.rpf`, 25,636 translated, 253 refused, 4.2 GB,
461 s on 6 threads. The cache key is the translator's fingerprint **plus**
`VICTORY_HOST_WAVE`, `VICTORY_VIEW_LIMIT`, `VICTORY_SHADER_LOOP_LIMIT`,
`VICTORY_SHADER_OPT` and the other `VICTORY_SHADER_*` switches, so the
warm-up must run with the same values the run will have, or every entry misses.

## Showing a target instead of the title's frame

`VICTORY_SHOW_TARGET=<w>x<h>-f<format>[-<n>]` presents the `n`th texture of
that size and format the frame renders to or writes (1-based, first-use
order) — from the proton-fixes branch of upstream issue #844. At frame 4517 of
this run, the 1280x720 targets were:

| index in the dump | format | luma (0-255) | what it looks like |
| --- | --- | --- | --- |
| `-09-0x452de1d0` | f28 | 98 | the scene shown by `-1` |
| `-43-0x452bd050` | f28 | 126 | a wider view of the same |
| `-104-0x458a2930` | f28 | 138 | brighter, later in the frame |
| `-187-0x143b60` | f62 | 145 | brightest of the 720p set |
| `-136-0x458b5f70` | f24 | 111 | |
| `-102-0x433e9b40` | f87 | 137 | |

`VICTORY_DUMP_TARGETS=1` with `VICTORY_CAPTURE=<folder>` writes every render
target of the captured frames as `frame-<n>-<i>-<resource>-<w>x<h>-f<f>.png`,
which is where this table comes from; the `i` is what `-<n>` counts.

## Patches this run rests on

- `abdb666` — the memory-map diagnostic walk is bounded (queries capped,
  back-off after a slow walk, none while free memory is under 512 MB: one walk
  took 24 s in run 93 at 231 MB of commit left) and `dxgi` reports the host's
  screen instead of a default 3840x2160.
- `5f60675` — fxceless' third frame-rate session (frame log, card meter,
  driver watch, no forbidden self-copy), merged from `a253706`.
- `e357311` — PR #188: a present's second plane is drawn over the first, and
  the upscaler's output is a plain stretch of its colour input (Series S).
- `3d8eb95`, `944ccfe` — from the proton-fixes branch of issue #844:
  `WSAConnectByList` / `GetAddrInfoExA` fail where the host lacks them, and an
  empty pipeline library is asked for with a null blob.
- `dd469b4`, `55a5db9` — that branch's `docs/linux-proton.md` and
  `scripts/proton-run.sh`.
- `bc58b19` — `VICTORY_SHOW_TARGET`, from the same branch.

Taken as already present: PR #1279's clamp (this tree's clamp is the embedded
DXIL one, "argument clamp: made from the embedded DXIL"), the heap-slot
mirroring of #844's `6f732b6` (this fork's `sync_heaps`, from upstream
86a5ed6), the write-watch commits of that branch (this fork's `watch.rs` is
newer, from `a253706`), and `VICTORY_PIPELINE_WAIT_MS` (already in
`pipeline/make.rs`).

## Still open here

1. The final 3840x2160 target stays zero: the second plane and the stretch run
   ("over: a present's second plane is drawn over", "the upscaler's output is
   made by a plain stretch of its colour input") but the composed target this
   fork presents is still the empty 4K one (upstream #2045: the lighting
   target is emptied by `CS_ApplyCombinedDeferred`; #841, #2027).
2. Textures in the world.
3. A tree-indexed wineserver: no toolchain on this machine (`autoconf`,
   `bison`, `flex`, `gcc`, `make` are all absent; installing them needs
   `steamos-readonly disable` and a network).
4. Microsoft's `d3dcompiler_47.dll`, x64: only the 32-bit copy from steamcmd is
   on this disk.

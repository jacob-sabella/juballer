# Rhythm mode (Phase 1)

A minimal rhythm-game inside `juballer-deck`. Loads a
[memon v1.0.0](https://memon-spec.readthedocs.io/en/latest/) chart, plays the
audio via `rodio`, renders approaching notes as per-tile WGSL shaders, judges
keypresses, and shows an HUD with combo/score.

## Running

```sh
cargo run --release -p juballer-deck --features raw-input -- \
    play assets/sample/test.memon --difficulty BSC
```

Or against the built binary:

```sh
./target/release/juballer-deck play path/to/chart.memon --difficulty ADV
```

Flags:

- `--difficulty <name>` — key under `data` in the memon JSON. Default `BSC`.
- `--audio-offset-ms <N>` — shift the master clock by `N` ms. Positive = audio
  lags input → subtracted from music-time so presses feel in sync.

## Exit

- `Esc` (if winit delivers it) or closing the window.
- Holding all four corners of the grid `(0,0) (0,3) (3,0) (3,3)` for 3 s.
- The song naturally ends and the final banner has been visible for 5 s.

On exit the mean signed input delta is logged:

```
rhythm: session end: score=12500 max_combo=18 P=10 GT=6 GD=2 PO=1 M=1 | mean input offset +18.3ms
```

A positive number means you're consistently late → try `--audio-offset-ms -18`
(or vice versa) on the next run.

## Sample chart

`assets/sample/test.memon` + `assets/sample/test.ogg` (a 30 s, 440 Hz sine
tone). The chart hits 30 notes across corners → inner cells → a diagonal row
sweep → a four-corner chord. Regenerate the audio with:

```sh
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=30" \
    -c:a libvorbis assets/sample/test.ogg
```

## Chart format (Phase 1 subset)

The full spec is at https://memon-spec.readthedocs.io. Phase 1 supports:

- `version` (must equal `"1.0.0"`)
- `metadata.title`, `metadata.artist`, `metadata.audio` (required)
- `timing.offset` (seconds, float), `timing.resolution` (int ticks/beat),
  `timing.bpms[0].bpm` (single constant BPM; later entries are ignored with
  a warn)
- `data.<DIFF>.notes[]` with integer `n ∈ 0..16` and integer `t` ticks
- Chart-level `timing` overrides top-level `timing`

Ignored:

- Long notes (`l`, `p` fields)
- BPM changes past the first entry
- Fractional tick form `[num, den, rem]` — skipped with a warn log
- `preview`, `jacket`, `hakus`

`n` maps to a grid cell via `row = n / 4, col = n % 4` (row-major; `n = 0` is
top-left, `n = 15` is bottom-right).

## Timing windows

Match rhythm-game defaults:

| Grade   | Window (±ms) | Score | Combo |
| ------- | ------------ | ----: | ----- |
| Perfect | 42           | 1000  | keeps |
| Great   | 82           |  500  | keeps |
| Good    | 125          |  100  | keeps |
| Poor    | 200          |   50  | breaks |
| Miss    | (> 200)      |    0  | breaks |

`judge(delta_ms)` lives in `crates/juballer-deck/src/rhythm/judge.rs`.

## Architecture

Rhythm mode is a peer to the DeckApp widget/action stack — it builds its own
`juballer_core::App`, runs its own `run` closure, and uses
`juballer_deck::shader::ShaderPipelineCache` directly for per-tile WGSL
rendering.

```
cli::SubCmd::Play ─► rhythm::play ─► App::run(move |frame, events| { ... })
                                         │
  rhythm::chart::load  ──► Chart         │
  rhythm::Audio        ──► master clock  │
  rhythm::GameState    ◄──── events ─────┤
                       ──► render slots ─┤
  rhythm::render       ──► shader + HUD ─┘
```

Input uses `Event::KeyDown.ts` (monotonic timestamp at the evdev/winit
boundary), not `Instant::now()` at event-handling time — this matters because
the frame callback may fire tens of ms after the keypress on a loaded system.

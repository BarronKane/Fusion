# Pico Bring-Up

This example now has one canonical firmware artifact and two transport paths:

- the canonical build product is the ELF
- `cargo pico-uf2` derives a sparse `.bin` plus `.uf2` for BOOTSEL flashing
- `cargo pico-flash`, `cargo pico-run`, and `cargo pico-attach` wrap the probe-rs path

## UF2 / BOOTSEL

From the workspace root:

```sh
cargo pico-uf2
```

That builds [main.rs](/volumes/projects/rust/fusion/Examples/PicoTarget/main.rs) through this
example's local linker configuration, then writes:

- `target/thumbv8m.main-none-eabihf/debug/pico.bin`
- `target/thumbv8m.main-none-eabihf/debug/pico.uf2`

Use `--release` after the `--` if you want a release image:

```sh
cargo pico-uf2 -- --release
```

Then hold `BOOTSEL`, plug in or reset the board, and copy `pico.uf2` onto the mounted drive.

## probe-rs / SWD

These commands require a physical SWD probe and either `probe-rs` or `cargo-flash` installed on
the host.

Probe wrappers require a chip name either on the command line or through the environment:

- `FUSION_PICO_PROBE_CHIP`
- `FUSION_PICO_PROBE_SELECTOR` (optional)

Examples:

```sh
FUSION_PICO_PROBE_CHIP=rp235x cargo pico-flash
FUSION_PICO_PROBE_CHIP=rp235x cargo pico-run
FUSION_PICO_PROBE_CHIP=rp235x cargo pico-attach
```

Or explicitly:

```sh
cargo pico-flash -- --chip rp235x
cargo pico-run -- --chip rp235x
cargo pico-attach -- --chip rp235x
```

The wrappers intentionally leave the chip string operator-supplied until first hardware validation
locks down the exact target spelling worth baking into repo defaults. Embedded life: all the fun
of being precise, none of the reward.

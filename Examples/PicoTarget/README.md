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
the host. The repo now carries one checked-in [Embed.toml](/volumes/projects/rust/fusion/Examples/PicoTarget/Embed.toml)
using the standard `RP235x` probe-rs target name from the upstream `rp-rs` RP235x template.

Probe wrappers default to `RP235x`. Override only if your local probe-rs build wants something
different:

- `FUSION_PICO_PROBE_CHIP` (optional override)
- `FUSION_PICO_PROBE_SELECTOR` (optional)

Examples:

```sh
cargo pico-flash
cargo pico-run
cargo pico-attach
```

Or explicitly:

```sh
cargo pico-flash -- --chip RP235x
cargo pico-run -- --chip RP235x
cargo pico-attach -- --chip RP235x
```

If you want the full probe-rs workflow instead of the thin wrapper, the checked-in
[Embed.toml](/volumes/projects/rust/fusion/Examples/PicoTarget/Embed.toml) is ready for:

```sh
cargo embed --release
```

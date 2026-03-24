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

## RustRover LLDB

The repo now checks in four primary shared run/debug targets so the normal RustRover Run widget
stays clean:

- `fusion-example-template debug hosted`
- `fusion-example-template release hosted`
- `fusion-example-pico debug on device`
- `fusion-example-pico release on device`

### Hosted example

The two `fusion-example-template ... hosted` profiles are normal Cargo run configurations for
[Examples/Template](/volumes/projects/rust/fusion/Examples/Template). Select either one in the
Run widget and use the normal IDE Run or Debug button.

### Pico on-device example

The two `fusion-example-pico ... on device` profiles are RustRover `Remote Debug`
configurations against a `probe-rs gdb` stub on `[::1]:2345`.

When you start one from the IDE, RustRover first runs a helper profile in `Before launch` that:

- builds the correct Pico ELF
- flashes it with `probe-rs download`
- starts a detached `probe-rs gdb --reset-halt` server

That means the normal IDE Debug button is the only thing you need for on-device debugging.

For pure on-device runs without attaching LLDB, this repo also checks in helper Cargo profiles
under the `fusion-example-pico helpers` folder in the Run/Debug configuration dialog.

### Helper profiles

The shared helper profiles live under [.run](/volumes/projects/rust/fusion/.run) and include:

- `helper fusion-example-pico debug on device run`
- `helper fusion-example-pico release on device run`
- `helper fusion-example-pico debug on device launch`
- `helper fusion-example-pico release on device launch`
- `helper fusion-example-pico rtt`
- `helper fusion-example-pico release flash`

The launch helpers are internal plumbing for the primary LLDB profiles. The run, RTT, and flash
helpers are still useful when you want explicit device control from the IDE.

### Notes

- The remote stub is GDB-remote-compatible because that is what `probe-rs` exposes today. LLDB is
  still the debugger frontend in RustRover.
- If you change the port, pass `-- --gdb-connection-string HOST:PORT` to `pico-debug-server` and
  update the remote command in the RustRover `Remote Debug` configuration to match.

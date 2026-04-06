# RP2350 Examples

- [MinimalLed](/volumes/projects/rust/fusion/Examples/Cortex-M/RP2350/MinimalLed): minimal LED example with fibers + async
- [Benchmark](/volumes/projects/rust/fusion/Examples/Cortex-M/RP2350/Benchmark): dedicated on-device benchmark firmware
- [Display](/volumes/projects/rust/fusion/Examples/Cortex-M/RP2350/Display): seven-segment display example using chained `74HC595`s

Workspace-root helpers:

```sh
cargo pico-build
cargo pico-flash -- --release
cargo pico-display-flash -- --release
cargo pico-benchmark -- --release
```

Probe-based `pico-*-flash`, `pico-*-run`, and benchmark/debug wrappers are expected to be the
canonical flashing path. They now build the scoped ELF, perform `probe-rs download --verify`,
reset the board, and then confirm the exported image build-id against the ELF they just built,
because apparently “the command returned” was not a strong enough definition of success for
embedded work.

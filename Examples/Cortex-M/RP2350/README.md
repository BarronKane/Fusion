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

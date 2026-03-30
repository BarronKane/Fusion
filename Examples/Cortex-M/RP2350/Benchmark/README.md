# RP2350 Benchmark Example

Dedicated on-device benchmark firmware for the Pico 2 W / RP2350 path.

- one current-thread fiber + async runtime backed by one exact-static owning slab
- example code calls the shared RP2350 on-device runtime surface directly with no local runtime
  standup
- stack admission now comes from generated contracts or explicit caller-supplied contracts rather
  than a shared backend floor

From the workspace root:

```sh
cargo pico-benchmark -- --release
```

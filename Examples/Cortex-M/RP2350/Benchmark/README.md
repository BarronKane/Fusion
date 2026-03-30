# RP2350 Benchmark Example

Dedicated on-device benchmark firmware for the Pico 2 W / RP2350 path.

- one current-thread fiber + async runtime backed by one exact-static owning slab
- generated stack metadata folded into the slab plan at build time

From the workspace root:

```sh
cargo pico-benchmark -- --release
```

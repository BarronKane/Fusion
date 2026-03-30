# RP2350 Display Example

Async-on-fiber RP2350 firmware that drives a four-digit multiplexed seven-segment display through
two chained `74HC595` shift registers.

The example now uses the shared RP2350 on-device runtime surface directly instead of a local
backend shim. Stack admission comes from generated contracts or explicit caller-supplied contracts;
there is no centralized board-default stack floor hiding in the shared backend anymore.

From the workspace root:

```sh
cargo pico-display-flash -- --release
```

# RP2350 Display Example

Async-on-fiber RP2350 firmware that drives a four-digit multiplexed seven-segment display through
two chained `74HC595` shift registers.

From the workspace root:

```sh
cargo pico-display-flash -- --release
```

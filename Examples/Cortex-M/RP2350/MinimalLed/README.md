# RP2350 Minimal LED Example

Minimal async-on-fiber LED firmware for the Pico 2 W / RP2350 path.

- startup dance on the two board LEDs
- async fizzbuzz pattern on top of one current-thread fiber runtime
- runtime backing preplanned from generated stack metadata into one exact-static owning slab
- red panic blink so the board stops failing silently

From the workspace root:

```sh
cargo pico-build
cargo pico-flash -- --release
```

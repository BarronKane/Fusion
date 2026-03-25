# RP2350 Minimal LED Example

Minimal async-on-fiber LED firmware for the Pico 2 / RP2350 path.

- startup dance on the two board LEDs
- async fizzbuzz pattern on top of one current-thread fiber runtime
- red panic blink so the board stops failing silently

From the workspace root:

```sh
cargo pico-build
cargo pico-flash -- --release
```

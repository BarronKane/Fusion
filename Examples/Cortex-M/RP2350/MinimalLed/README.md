# RP2350 Minimal LED Example

Minimal async-on-fiber LED firmware for the Pico 2 W / RP2350 path.

- startup dance on the two board LEDs
- async fizzbuzz pattern on top of one current-thread fiber runtime
- example code calls the shared RP2350 on-device runtime surface directly with no local runtime
  standup
- async poll-stack admission now comes from generated contracts or explicit caller-supplied
  contracts rather than a shared backend floor
- red panic blink so the board stops failing silently

From the workspace root:

```sh
cargo pico-build
cargo pico-flash -- --release
```

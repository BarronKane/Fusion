# RP2350 DebugChannel Example

Cloned from the display example as a separate RP2350 firmware lane for aggressive channel/fiber
debugging without turning the ordinary display example back into a trench warfare artifact.

From the workspace root:

```sh
cargo pico-debug-channel-flash -- --release
```

That alias is the intended probe path. It builds the scoped ELF, performs `probe-rs download
--verify`, resets the board into the flashed image, and then compares the exported build-id on the
target against the ELF it just built so the deploy step stops pretending optimism is verification.

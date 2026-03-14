# fusion-kn

`fusion-kn` is the beginning of Fusion's kernel-facing crate.

It is intentionally a blueprint first:

- a Cargo-visible crate for kernel-facing policy and evidence vocabulary
- a Rust-for-Linux out-of-tree build seam via `Kbuild` and `Makefile`
- a place to record allocation, panic, unsafe-boundary, and initialization discipline
  before real kernel code arrives

This crate does not claim DO-178C compliance today. It starts the structural work needed for
an eventual assurance story instead of bolting that on after the kernel boundary already
exists.

The initial Linux out-of-tree build seam is modeled on the Rust-for-Linux
`rust-out-of-tree-module` template.

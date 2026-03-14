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

The current kernel module entry is intentionally inert. It registers no user-facing
device or sysfs surface yet. That is deliberate: the first real deliverable for
`fusion-kn` is a strict kernel-boundary contract, not a demo interface.

The Cargo-visible crate now carries:

- kernel integration metadata and build requirements
- a strict boundary contract describing allowed contexts, blocking policy, panic policy,
  allocation policy, and explicitly reviewed boundary crossings
- a fixed-layout mediated wire protocol for negotiated kernel/user exchange
- a no-alloc client surface that can be consumed by `fusion-pal`
- evidence-planning vocabulary for a future assurance story

Only after those rules are stable should user-facing kernel surfaces start to appear.

Feature split:

- `contract`: shared boundary policy, blueprint, evidence, and wire vocabulary
- `client`: no-alloc protocol client helpers for mediated backends
- `module`: Rust-for-Linux out-of-tree module build path

The build script is intentionally inert unless `module` is enabled. That keeps
client-side consumers from accidentally trying to build a kernel module just because
Cargo woke up feeling malicious.

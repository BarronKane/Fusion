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

The current sample module registers a flat misc-device node named
`/dev/fusion_kn_hello_world`. A sample `udev` rule is provided in
[`99-fusion-kn.rules`](./99-fusion-kn.rules) to expose a friendlier
`/dev/fusion-kn/hello_world` symlink with group-readable permissions:

1. Install the rule to `/etc/udev/rules.d/99-fusion-kn.rules`.
2. Create a system group: `sudo groupadd --system fusionkn`
3. Add intended users: `sudo usermod -aG fusionkn "$USER"`
4. Reload rules: `sudo udevadm control --reload`
5. Trigger the misc device or reload the module.

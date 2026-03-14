// SPDX-License-Identifier: GPL-2.0

//! Minimal Rust-for-Linux misc-device entrypoint for `fusion-kn`.
//!
//! This is intentionally tiny. It gives us a real kernel build seam and a visible device
//! endpoint without pretending the surrounding kernel contract story is finished.
//! Userspace policy can then expose a friendlier `/dev/fusion-kn/hello_world` symlink via
//! `udev` without forcing slashes into the kernel-facing device node name.

use kernel::{
    device::Device,
    fs::{File, Kiocb},
    iov::IovIterDest,
    miscdevice::{MiscDevice, MiscDeviceOptions, MiscDeviceRegistration},
    prelude::*,
    sync::aref::ARef,
};

module! {
    type: FusionKnModule,
    name: "fusion_kn",
    authors: ["Lance Wallis"],
    description: "Fusion kernel hello world misc device",
    license: "GPL",
}

const HELLO_WORLD: &[u8] = b"hello world\n";

#[pin_data]
struct FusionKnModule {
    #[pin]
    _miscdev: MiscDeviceRegistration<FusionKnHelloWorld>,
}

impl kernel::InPlaceModule for FusionKnModule {
    fn init(_module: &'static ThisModule) -> impl PinInit<Self, Error> {
        pr_info!(
            "fusion-kn: registering /dev/fusion_kn_hello_world (udev may add /dev/fusion-kn/hello_world)\n"
        );

        let options = MiscDeviceOptions {
            name: c"fusion_kn_hello_world",
        };

        try_pin_init!(Self {
            _miscdev <- MiscDeviceRegistration::register(options),
        })
    }
}

#[pin_data(PinnedDrop)]
struct FusionKnHelloWorld {
    dev: ARef<Device>,
}

#[vtable]
impl MiscDevice for FusionKnHelloWorld {
    type Ptr = Pin<KBox<Self>>;

    fn open(_file: &File, misc: &MiscDeviceRegistration<Self>) -> Result<Pin<KBox<Self>>> {
        let dev = ARef::from(misc.device());

        dev_info!(dev, "fusion-kn hello_world opened\n");

        KBox::try_pin_init(
            try_pin_init! {
                FusionKnHelloWorld {
                    dev: dev,
                }
            },
            GFP_KERNEL,
        )
    }

    fn read_iter(mut kiocb: Kiocb<'_, Self::Ptr>, iov: &mut IovIterDest<'_>) -> Result<usize> {
        let me = kiocb.file();

        dev_info!(me.dev, "fusion-kn hello_world read\n");

        iov.simple_read_from_buffer(kiocb.ki_pos_mut(), HELLO_WORLD)
    }
}

#[pinned_drop]
impl PinnedDrop for FusionKnHelloWorld {
    fn drop(self: Pin<&mut Self>) {
        dev_info!(self.dev, "fusion-kn hello_world closed\n");
    }
}

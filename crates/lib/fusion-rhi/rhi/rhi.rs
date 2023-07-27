use std::convert::TryFrom;

enum VendorID {
    AMD,
    NVIDIA,
    APPLE,
    INTEL,
    ARM,
    QUALCOMM,
    BROADCOM,
    IMGTEC,
    MESA,
}

impl VendorID {
    pub fn id(vendor_id: VendorID) -> u32 {
        match vendor_id {
            VendorID::AMD => 0x1002,
            VendorID::NVIDIA => 0x10DE,
            VendorID::APPLE => 0x106B,
            VendorID::INTEL => 0x8086,
            VendorID::ARM => 0x13B5,
            VendorID::QUALCOMM => 0x5143,
            VendorID::BROADCOM => 0x14E4,
            VendorID::IMGTEC => 0x1010,
            VendorID::MESA => 0x10005,
        }
    }
}

impl TryFrom<u32> for VendorID {
    type Error = ();

    fn try_from(v: u32) -> Result<Self, Self::Error> {
        match v {
            0x1002 => Ok(VendorID::AMD),
            0x10DE => Ok(VendorID::NVIDIA),
            0x106B => Ok(VendorID::APPLE),
            0x8086 => Ok(VendorID::INTEL),
            0x13B5 => Ok(VendorID::ARM),
            0x5143 => Ok(VendorID::QUALCOMM),
            0x14E4 => Ok(VendorID::BROADCOM),
            0x1010 => Ok(VendorID::IMGTEC),

            // Bellow this comment use the VkVendorIDs.
            0x10005 => Ok(VendorID::MESA),


            _ => Err(())
        }
    }
}

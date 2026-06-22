use rand::Rng;
use std::fmt;
use std::str::FromStr;

/// A 48-bit Ethernet MAC address.
///
/// UTM uses QEMU's locally-administered range (`52:54:00:xx:xx:xx`) by convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    /// Create a MAC from raw octets.
    pub fn from_octets(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    /// Generate a random MAC in QEMU's locally-administered range (`52:54:00:xx:xx:xx`).
    pub fn random_qemu() -> Self {
        let mut rng = rand::thread_rng();
        Self([0x52, 0x54, 0x00, rng.gen(), rng.gen(), rng.gen()])
    }

    /// Return the MAC as a colon-separated lowercase string (e.g. `52:54:00:ab:cd:ef`).
    pub fn as_str(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for MacAddress {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return Err(crate::Error::Other(format!("invalid MAC address: {s}")));
        }
        let mut octets = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            octets[i] = u8::from_str_radix(part, 16)
                .map_err(|_| crate::Error::Other(format!("invalid MAC octet '{part}' in '{s}'")))?;
        }
        Ok(Self(octets))
    }
}
//! Library facade for solver-main. Holds shared helpers (MESSIAH keychain
//! bootstrap, etc.) that the `taifoon-solver` and `estimate_one` binaries
//! both consume.

pub mod lifi_resolver;
pub mod messiah;

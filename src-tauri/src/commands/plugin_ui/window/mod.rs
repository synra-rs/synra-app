use super::*;

/// Page bootstrap and navigation policy.
mod injection;
/// Window admission and reservation.
mod open;
/// Bundle resources and document responses.
mod resource;
#[cfg(feature = "tauri-shell")]
/// Tauri protocol serving.
mod serve;

pub use injection::OpenRequest;
pub use open::*;
pub use resource::*;
#[cfg(feature = "tauri-shell")]
pub use serve::serve;

#[cfg(any(test, feature = "tauri-shell"))]
pub(crate) use injection::stays_on_page;
pub(crate) use injection::{
    FACADE, InjectionSeed, initialization_script, platform_capability, platform_name,
};
#[cfg(feature = "tauri-shell")]
pub(crate) use resource::document_response;

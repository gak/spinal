use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

static NEXT_ASSET_KEY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct AssetKey(NonZeroU64);

impl AssetKey {
    pub(crate) fn try_fresh() -> Option<Self> {
        let value = NEXT_ASSET_KEY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .ok()?;
        NonZeroU64::new(value).map(Self)
    }
}

macro_rules! define_id {
    ($(#[$implementation_attribute:meta])* $name:ident, $documentation:literal) => {
        #[doc = $documentation]
        ///
        /// IDs are scoped to the loaded asset that created them. They are not
        /// stable across reloads or processes and cannot be constructed by
        /// callers.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name {
            asset: AssetKey,
            index: u32,
        }

        $(#[$implementation_attribute])*
        impl $name {
            pub(crate) const fn new(asset: AssetKey, index: u32) -> Self {
                Self { asset, index }
            }

            pub(crate) const fn asset(self) -> AssetKey {
                self.asset
            }

            pub(crate) const fn index(self) -> u32 {
                self.index
            }
        }
    };
}

define_id!(BoneId, "An asset-scoped bone identifier.");
define_id!(SlotId, "An asset-scoped slot identifier.");
define_id!(SkinId, "An asset-scoped skin identifier.");
define_id!(AttachmentId, "An asset-scoped attachment identifier.");
define_id!(AnimationId, "An asset-scoped animation identifier.");
define_id!(EventId, "An asset-scoped animation-event identifier.");
define_id!(
    IkConstraintId,
    "An asset-scoped inverse-kinematics constraint identifier."
);
define_id!(
    ConstraintId,
    "An asset-scoped identifier for any authored constraint."
);
define_id!(
    AtlasPageId,
    "An asset-scoped texture-atlas page identifier."
);
define_id!(
    AtlasRegionId,
    "An asset-scoped texture-atlas region identifier."
);

/// Why an asset-scoped identifier could not be resolved.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum IdErrorKind {
    /// The identifier belongs to a different loaded asset.
    #[error("the identifier belongs to a different asset")]
    ForeignAsset,
}

/// An error returned when an identifier is used with a different asset.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct IdError {
    kind: IdErrorKind,
}

impl IdError {
    /// Returns the stable category of identifier failure.
    #[must_use]
    pub const fn kind(self) -> IdErrorKind {
        self.kind
    }

    pub(crate) const fn foreign_asset() -> Self {
        Self {
            kind: IdErrorKind::ForeignAsset,
        }
    }
}

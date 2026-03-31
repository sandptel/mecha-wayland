//! # Asset Manager
//!
//! A typed, handle-based asset management system. Assets are self-loading —
//! each asset type defines how to construct itself from its parameters via the
//! [`Asset`] trait. The [`AssetManager`] provides centralized storage with
//! typed per-asset-type stores, keyed by opaque [`Handle`]s.
//!
//! Stores are created lazily on first load, so no upfront registration is needed.
//!
//! ## Architecture
//!
//! ```text
//! AssetManager
//! ├── AssetStore<ImageRaw>   ── HashMap<AssetId, ImageRaw>
//! ├── AssetStore<FontRaw>    ── HashMap<AssetId, FontRaw>
//! └── AssetStore<SvgData>    ── HashMap<AssetId, SvgData>
//! ```
//!
//! Internally, stores are type-erased behind `dyn Any` at the container level,
//! but individual assets are never type-erased — all access is fully typed
//! through the [`Handle<A>`] returned at load time.

use anyhow::Result;
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Opaque identifier for a loaded asset.
///
/// Two `AssetId`s are equal only if they refer to the same asset instance.
/// IDs are never reused within the lifetime of an [`AssetManager`].
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AssetId(usize);

/// A typed, lightweight reference to a loaded asset.
///
/// Handles are cheap to copy and carry no ownership semantics — the
/// [`AssetManager`] owns the actual asset data. A handle is only valid
/// for the manager that produced it.
#[derive(Debug)]
pub struct Handle<A: Asset> {
    asset_id: AssetId,
    _phantom: std::marker::PhantomData<A>,
}

impl<A: Asset> Clone for Handle<A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A: Asset> Copy for Handle<A> {}

impl<A: Asset> PartialEq for Handle<A> {
    fn eq(&self, other: &Self) -> bool {
        self.asset_id == other.asset_id
    }
}

impl<A: Asset> Eq for Handle<A> {}

impl<A: Asset> std::hash::Hash for Handle<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.asset_id.hash(state);
    }
}

impl<A: Asset> Handle<A> {
    fn new(asset_id: AssetId) -> Self {
        Self {
            asset_id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the underlying [`AssetId`].
    pub fn id(&self) -> AssetId {
        self.asset_id
    }
}

/// A self-loading asset.
///
/// Implementors define their own loading logic via [`Asset::load`]. The
/// associated [`Params`](Asset::Params) type describes what information
/// is needed to load the asset (typically a file path, but can be anything).
///
/// The `load` function is a static constructor — it receives parameters and
/// returns a fully constructed asset. For assets that require GPU resources,
/// `load` should produce a raw/CPU-side representation; the renderer can
/// then prepare GPU resources separately during its prepare pass.
pub trait Asset: Sized + 'static {
    /// Parameters required to load this asset.
    ///
    /// Common choices: `PathBuf` for file-backed assets, a custom params
    /// struct for assets that need additional configuration, or `()` for
    /// assets constructed without external input.
    type Params: 'static;

    /// Construct this asset from the given parameters.
    ///
    /// This should handle all CPU-side work: reading bytes from disk,
    /// decoding, parsing, etc. It must not depend on any GPU or
    /// renderer state.
    fn load(params: Self::Params) -> Result<Self>;
}

// ── Internal: per-type storage ──────────────────────────────────────────────

/// Typed storage for a single asset type.
struct AssetStore<A: Asset> {
    storage: HashMap<AssetId, A>,
}

/// Type-erased trait so we can hold heterogeneous stores in a single map.
///
/// `Any` is used *only* at this container level to look up the right
/// `AssetStore<A>`. Individual assets are never type-erased.
trait ErasedStore: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<A: Asset> ErasedStore for AssetStore<A> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Asset Manager ───────────────────────────────────────────────────────────

/// Centralized asset storage and loading.
///
/// The manager holds one [`AssetStore<A>`] per registered asset type.
/// Stores are created lazily on first [`load`](AssetManager::load) call,
/// so no upfront registration is required.
///
/// All public methods are fully typed through the [`Asset`] trait bound —
/// there is no runtime type confusion possible at the call site.
pub struct AssetManager {
    stores: HashMap<TypeId, Box<dyn ErasedStore>>,
    next_id: usize,
}

impl AssetManager {
    /// Create a new, empty asset manager.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            next_id: 0,
        }
    }

    /// Generate the next unique [`AssetId`].
    fn next_id(&mut self) -> AssetId {
        let id = AssetId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Get or create the typed store for asset type `A`.
    fn store_mut<A: Asset>(&mut self) -> &mut AssetStore<A> {
        self.stores
            .entry(TypeId::of::<A>())
            .or_insert_with(|| {
                Box::new(AssetStore::<A> {
                    storage: HashMap::new(),
                })
            })
            .as_any_mut()
            .downcast_mut::<AssetStore<A>>()
            .expect("AssetStore type mismatch — this is a bug")
    }

    /// Get the typed store for asset type `A`, if it exists.
    fn store<A: Asset>(&self) -> Option<&AssetStore<A>> {
        self.stores
            .get(&TypeId::of::<A>())
            .and_then(|s| s.as_any().downcast_ref::<AssetStore<A>>())
    }

    /// Load an asset from the given parameters.
    ///
    /// Calls [`Asset::load`] to construct the asset, stores it, and returns
    /// a typed [`Handle`]. The parameter type `P` can be anything that
    /// converts into the asset's [`Params`](Asset::Params) type via `Into`.
    ///
    /// # Errors
    ///
    /// Returns an error if the asset's `load` function fails (e.g. file not
    /// found, decode error).
    pub fn load<A, P>(&mut self, params: P) -> Result<Handle<A>>
    where
        A: Asset,
        P: Into<A::Params>,
    {
        let asset = A::load(params.into())?;
        let id = self.next_id();
        self.store_mut::<A>().storage.insert(id, asset);
        Ok(Handle::new(id))
    }

    /// Insert a pre-constructed asset directly into the manager.
    ///
    /// Useful for assets that were constructed outside the normal `load` path,
    /// such as procedurally generated data or assets received over the network.
    pub fn insert<A: Asset>(&mut self, asset: A) -> Handle<A> {
        let id = self.next_id();
        self.store_mut::<A>().storage.insert(id, asset);
        Handle::new(id)
    }

    /// Get a shared reference to a loaded asset.
    ///
    /// Returns `None` if the handle does not correspond to a stored asset
    /// (e.g. it was removed, or belongs to a different manager).
    pub fn get<A: Asset>(&self, handle: &Handle<A>) -> Option<&A> {
        self.store::<A>()?.storage.get(&handle.id())
    }

    /// Get an exclusive reference to a loaded asset.
    pub fn get_mut<A: Asset>(&mut self, handle: &Handle<A>) -> Option<&mut A> {
        self.store_mut::<A>().storage.get_mut(&handle.id())
    }

    /// Remove an asset from the manager, returning it if it existed.
    pub fn remove<A: Asset>(&mut self, handle: &Handle<A>) -> Option<A> {
        self.store_mut::<A>().storage.remove(&handle.id())
    }

    /// Returns the number of loaded assets of type `A`.
    pub fn count<A: Asset>(&self) -> usize {
        self.store::<A>().map_or(0, |s| s.storage.len())
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

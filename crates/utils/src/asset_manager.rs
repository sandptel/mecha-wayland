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
//! ├── stores:    TypeId → Box<dyn Any>              (AssetStore<A> per asset type)
//! └── processed: TypeId → Box<dyn ErasedProcessedMap>  (HashMap<AssetId, O> per output type)
//! ```
//!
//! Both maps are keyed by `TypeId` and store their values as type-erased boxes,
//! with a single downcast at the access site. Individual assets are never
//! type-erased — all access is fully typed through the [`Handle<A>`] returned
//! at load time.

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

    /// Get the post-processed output for this handle.
    ///
    /// Equivalent to [`AssetManager::get_processed`] but infers the asset type
    /// from `self`, so only the output type `O` needs to be specified:
    ///
    /// ```ignore
    /// let gpu_image = logo_handle.get_processed::<GpuImage>(&assets).unwrap();
    /// ```
    pub fn get_processed<'m, O: 'static>(&self, manager: &'m AssetManager) -> Option<&'m O> {
        manager.get_processed::<A, O>(self)
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

/// A post-processing step that converts a CPU-side asset into another form.
///
/// Implement this in crates that have access to the necessary context (e.g.
/// a GPU context) without creating a circular dependency on `utils`.
///
/// # Example
///
/// ```ignore
/// // In the renderer crate:
/// impl AssetPostProcessor for GpuImageProcessor<'_> {
///     type Input = ImageAsset;
///     type Output = GpuImage;
///     fn process(&mut self, asset: &ImageAsset) -> Result<GpuImage> { ... }
/// }
///
/// // At init time:
/// asset_manager.process_pending(&mut renderer.image_processor())?;
/// let gpu_image = logo_handle.get_processed::<GpuImage>(&asset_manager).unwrap();
/// ```
pub trait AssetPostProcessor {
    /// The CPU-side asset type this processor consumes.
    type Input: Asset;
    /// The output type produced by this processor (e.g. `GpuImage`).
    type Output: 'static;
    fn process(&mut self, asset: &Self::Input) -> Result<Self::Output>;
}

// ── Internal: type-erased processed map ────────────────────────────────────

/// Type-erased interface for `HashMap<AssetId, O>` stored in `processed`.
///
/// Allows [`AssetManager::remove`] to iterate all output maps and clean up
/// by id without knowing the concrete output type `O`.
trait ErasedProcessedMap: Any {
    fn remove_id(&mut self, id: AssetId);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<O: 'static> ErasedProcessedMap for HashMap<AssetId, O> {
    fn remove_id(&mut self, id: AssetId) {
        self.remove(&id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Internal: per-type storage ──────────────────────────────────────────────

/// Typed storage for a single asset type.
struct AssetStore<A: Asset> {
    storage: HashMap<AssetId, A>,
    pending: Vec<AssetId>,
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
    /// One `AssetStore<A>` per asset type, type-erased to `Box<dyn Any>`.
    stores: HashMap<TypeId, Box<dyn Any>>,
    /// Post-processed outputs: `TypeId::of::<O>()` → `HashMap<AssetId, O>`.
    processed: HashMap<TypeId, Box<dyn ErasedProcessedMap>>,
    next_id: usize,
}

impl AssetManager {
    /// Create a new, empty asset manager.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            processed: HashMap::new(),
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
                    pending: Vec::new(),
                }) as Box<dyn Any>
            })
            .downcast_mut::<AssetStore<A>>()
            .expect("AssetStore type mismatch — this is a bug")
    }

    /// Get the typed store for asset type `A`, if it exists.
    fn store<A: Asset>(&self) -> Option<&AssetStore<A>> {
        self.stores
            .get(&TypeId::of::<A>())
            .and_then(|s| s.downcast_ref::<AssetStore<A>>())
    }

    // ── Core CRUD ────────────────────────────────────────────────────────────

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
        let store = self.store_mut::<A>();
        store.storage.insert(id, asset);
        store.pending.push(id);
        Ok(Handle::new(id))
    }

    /// Insert a pre-constructed asset directly into the manager.
    ///
    /// Useful for assets that were constructed outside the normal `load` path,
    /// such as procedurally generated data or assets received over the network.
    pub fn insert<A: Asset>(&mut self, asset: A) -> Handle<A> {
        let id = self.next_id();
        let store = self.store_mut::<A>();
        store.storage.insert(id, asset);
        store.pending.push(id);
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

    /// Remove an asset and all its post-processed outputs from the manager.
    ///
    /// Iterates every output map in `processed` and removes the corresponding
    /// entry, so no outputs are orphaned regardless of how many processors
    /// have run on this asset.
    pub fn remove<A: Asset>(&mut self, handle: &Handle<A>) -> Option<A> {
        let id = handle.id();
        for map in self.processed.values_mut() {
            map.remove_id(id);
        }
        self.store_mut::<A>().storage.remove(&id)
    }

    /// Returns the number of loaded assets of type `A`.
    pub fn count<A: Asset>(&self) -> usize {
        self.store::<A>().map_or(0, |s| s.storage.len())
    }

    /// Returns the number of assets of type `A` awaiting post-processing.
    pub fn pending_count<A: Asset>(&self) -> usize {
        self.store::<A>().map_or(0, |s| s.pending.len())
    }

    /// Iterate over handles to all loaded assets of type `A`.
    pub fn iter_handles<A: Asset>(&self) -> impl Iterator<Item = Handle<A>> + '_ {
        self.store::<A>()
            .into_iter()
            .flat_map(|s| s.storage.keys().copied().map(Handle::new))
    }

    // ── Post-processing ──────────────────────────────────────────────────────

    fn processed_map_mut<O: 'static>(&mut self) -> &mut HashMap<AssetId, O> {
        self.processed
            .entry(TypeId::of::<O>())
            .or_insert_with(|| {
                Box::new(HashMap::<AssetId, O>::new()) as Box<dyn ErasedProcessedMap>
            })
            .as_any_mut()
            .downcast_mut::<HashMap<AssetId, O>>()
            .expect("processed map type mismatch — this is a bug")
    }

    fn processed_map<O: 'static>(&self) -> Option<&HashMap<AssetId, O>> {
        self.processed
            .get(&TypeId::of::<O>())?
            .as_any()
            .downcast_ref()
    }

    /// Run `processor` over every asset that has not yet been processed.
    ///
    /// Returns one `Result<()>` per pending asset. Failures are collected
    /// rather than short-circuiting, so a single bad asset does not prevent
    /// the rest from being processed. Calling this with no new loads returns
    /// an empty `Vec`.
    ///
    /// Processed outputs are stored internally and can be retrieved via
    /// [`get_processed`](Self::get_processed) or [`Handle::get_processed`].
    pub fn process_pending<Proc>(&mut self, processor: &mut Proc) -> Vec<Result<()>>
    where
        Proc: AssetPostProcessor,
    {
        // Drain the pending list before the loop to avoid holding a mutable
        // borrow on `stores` while we also write to `processed`.
        let pending: Vec<AssetId> = {
            let store = self.store_mut::<Proc::Input>();
            store.pending.drain(..).collect()
        };

        // Phase 1: shared borrow of `stores` — read assets, run processor.
        // Outputs are owned values so the borrow ends before phase 2.
        let results: Vec<(AssetId, Result<Proc::Output>)> = pending
            .into_iter()
            .filter_map(|id| {
                self.store::<Proc::Input>()
                    .and_then(|s| s.storage.get(&id))
                    .map(|asset| (id, processor.process(asset)))
            })
            .collect();

        // Phase 2: mutable borrow of `processed` — insert successful outputs.
        results
            .into_iter()
            .map(|(id, result)| {
                result.map(|output| {
                    self.processed_map_mut::<Proc::Output>().insert(id, output);
                })
            })
            .collect()
    }

    /// Load an asset and immediately post-process it in one call.
    ///
    /// Unlike [`load`](Self::load) followed by [`process_pending`](Self::process_pending),
    /// this method bypasses the pending queue entirely. The asset is loaded,
    /// processed, and both the CPU and output forms are stored before this
    /// call returns.
    ///
    /// # Errors
    ///
    /// Returns an error if either [`Asset::load`] or [`AssetPostProcessor::process`] fails.
    pub fn load_and_process<P, Proc>(
        &mut self,
        params: P,
        processor: &mut Proc,
    ) -> Result<Handle<Proc::Input>>
    where
        Proc: AssetPostProcessor,
        P: Into<<Proc::Input as Asset>::Params>,
    {
        let asset = Proc::Input::load(params.into())?;
        let id = self.next_id();
        let output = processor.process(&asset)?;
        // Not pushed to `pending` — already processed.
        self.store_mut::<Proc::Input>().storage.insert(id, asset);
        self.processed_map_mut::<Proc::Output>().insert(id, output);
        Ok(Handle::new(id))
    }

    /// Get the post-processed output `O` for the given handle.
    ///
    /// Returns `None` if [`process_pending`](Self::process_pending) has not yet
    /// been called for this asset, or if the asset was not found.
    ///
    /// If you already have a `Handle<A>`, prefer [`Handle::get_processed`] —
    /// it infers the asset type and only requires specifying `O`.
    pub fn get_processed<A: Asset, O: 'static>(&self, handle: &Handle<A>) -> Option<&O> {
        self.processed_map::<O>()?.get(&handle.asset_id)
    }

    /// Get a mutable reference to a post-processed output.
    pub fn get_processed_mut<A: Asset, O: 'static>(
        &mut self,
        handle: &Handle<A>,
    ) -> Option<&mut O> {
        self.processed_map_mut::<O>().get_mut(&handle.asset_id)
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

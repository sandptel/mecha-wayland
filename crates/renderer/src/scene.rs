use std::any::TypeId;
use std::collections::HashMap;

/// Type-erased layer: raw instance bytes + count + optional bound texture.
pub struct PrimitiveLayer {
    pub instances: Vec<u8>,
    pub count:     usize,
    pub texture:   Option<glow::Texture>,
}

impl PrimitiveLayer {
    fn new() -> Self {
        Self { instances: Vec::new(), count: 0, texture: None }
    }
}

/// Uniquely identifies a primitive within a scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrimitiveId(pub u64);

pub struct Scene {
    pub background: (f32, f32, f32),
    layers: HashMap<TypeId, PrimitiveLayer>,
    next_id: u64,
}

impl Scene {
    pub fn new() -> Self {
        Self { background: (0.0, 0.0, 0.0), layers: HashMap::new(), next_id: 0 }
    }

    /// Push raw instance bytes for a given primitive type. Called by
    /// `RenderablePrimitive::add_to_scene` implementations.
    pub fn push_raw(
        &mut self,
        type_id: TypeId,
        bytes: &[u8],
        texture: Option<glow::Texture>,
    ) -> PrimitiveId {
        let layer = self.layers.entry(type_id).or_insert_with(PrimitiveLayer::new);
        layer.instances.extend_from_slice(bytes);
        layer.count += 1;
        if texture.is_some() {
            layer.texture = texture;
        }
        let id = PrimitiveId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn get_layer(&self, type_id: TypeId) -> Option<&PrimitiveLayer> {
        self.layers.get(&type_id)
    }

    /// Clear all primitive data for the next frame. Does not reset background.
    pub fn clear_primitives(&mut self) {
        for layer in self.layers.values_mut() {
            layer.instances.clear();
            layer.count = 0;
            layer.texture = None;
        }
    }
}

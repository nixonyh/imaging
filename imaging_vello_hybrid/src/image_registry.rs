// Copyright 2026 the Imaging Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::Error;
use peniko::{ImageBrush, ImageData};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use vello_common::paint::{Image as VelloImage, ImageId, ImageSource};

#[derive(Debug, Default)]
pub(crate) struct HybridImageRegistry {
    live: HashMap<ImageKey, ImageId>,
}

impl HybridImageRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_upload_session<'a>(
        &'a mut self,
        renderer: &'a mut vello_hybrid::Renderer,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        encoder: wgpu::CommandEncoder,
    ) -> HybridImageUploadSession<'a> {
        HybridImageUploadSession {
            registry: self,
            renderer,
            device,
            queue,
            encoder: Some(encoder),
            pending: HashMap::new(),
        }
    }

    pub(crate) fn clear(
        &mut self,
        renderer: &mut vello_hybrid::Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        for image_id in self.live.drain().map(|(_, image_id)| image_id) {
            renderer.destroy_image(device, queue, encoder, image_id);
        }
    }
}

pub(crate) struct HybridImageUploadSession<'a> {
    registry: &'a mut HybridImageRegistry,
    renderer: &'a mut vello_hybrid::Renderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: Option<wgpu::CommandEncoder>,
    pending: HashMap<ImageKey, ImageId>,
}

impl HybridImageUploadSession<'_> {
    pub(crate) fn resolve_image_brush(&mut self, brush: &ImageBrush) -> Result<VelloImage, Error> {
        let key = ImageKey::derive(&brush.image);
        let image_id = if let Some(image_id) = self.pending.get(&key).copied() {
            image_id
        } else if let Some(image_id) = self.registry.live.get(&key).copied() {
            image_id
        } else {
            let image_source = ImageSource::from_peniko_image_data(&brush.image);
            let ImageSource::Pixmap(pixmap) = image_source else {
                return Err(Error::Internal(
                    "peniko image conversion did not produce a pixmap",
                ));
            };
            let image_id = self.renderer.upload_image(
                self.device,
                self.queue,
                self.encoder
                    .as_mut()
                    .expect("hybrid image upload session should own an encoder"),
                &pixmap,
            );
            self.pending.insert(key, image_id);
            image_id
        };

        Ok(VelloImage {
            image: ImageSource::OpaqueId(image_id),
            sampler: brush.sampler,
        })
    }

    pub(crate) fn finish(&mut self, success: bool) {
        if success {
            self.registry.live.extend(self.pending.drain());
        } else {
            for image_id in self.pending.drain().map(|(_, image_id)| image_id) {
                self.renderer.destroy_image(
                    self.device,
                    self.queue,
                    self.encoder
                        .as_mut()
                        .expect("hybrid image upload session should own an encoder"),
                    image_id,
                );
            }
        }

        if let Some(encoder) = self.encoder.take() {
            self.queue.submit([encoder.finish()]);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ImageKey {
    format: core::mem::Discriminant<peniko::ImageFormat>,
    alpha_type: core::mem::Discriminant<peniko::ImageAlphaType>,
    width: u32,
    height: u32,
    data_hash: u64,
}

impl ImageKey {
    fn derive(image: &ImageData) -> Self {
        let mut hasher = DefaultHasher::new();
        image.data.data().hash(&mut hasher);
        Self {
            format: core::mem::discriminant(&image.format),
            alpha_type: core::mem::discriminant(&image.alpha_type),
            width: image.width,
            height: image.height,
            data_hash: hasher.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ImageKey;
    use peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
    use std::sync::Arc;

    fn image(bytes: [u8; 16]) -> ImageData {
        ImageData {
            data: Blob::new(Arc::new(bytes)),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: 2,
            height: 2,
        }
    }

    #[test]
    fn image_key_dedupes_equivalent_image_contents() {
        let a = image([1, 2, 3, 4, 9, 8, 7, 6, 5, 4, 3, 2, 10, 11, 12, 13]);
        let b = image([1, 2, 3, 4, 9, 8, 7, 6, 5, 4, 3, 2, 10, 11, 12, 13]);
        assert_eq!(ImageKey::derive(&a), ImageKey::derive(&b));
    }

    #[test]
    fn image_key_distinguishes_metadata_changes() {
        let mut a = image([1, 2, 3, 4, 9, 8, 7, 6, 5, 4, 3, 2, 10, 11, 12, 13]);
        let mut b = a.clone();
        b.alpha_type = ImageAlphaType::AlphaPremultiplied;
        assert_ne!(ImageKey::derive(&a), ImageKey::derive(&b));

        a.format = ImageFormat::Bgra8;
        assert_ne!(ImageKey::derive(&a), ImageKey::derive(&b));
    }
}

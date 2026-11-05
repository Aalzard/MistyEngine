//! GPU copies of runtime textures (video frames, see [`crate::video::textures`]): one texture
//! per name, re-uploaded when a new frame is published. Names nothing was published under yet
//! show black.

use std::collections::HashMap;

use crate::video::textures as frames;

struct Entry {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    version: u64,
    size: (u32, u32),
}

#[derive(Default)]
pub(crate) struct RuntimeTextures {
    map: HashMap<String, Entry>,
}

fn create(device: &wgpu::Device, name: &str, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(name),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, w: u32, h: u32, pixels: &[u8]) {
    queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

impl RuntimeTextures {
    /// The view for `name`, created (with its latest frame, or black) on first use.
    pub fn view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
    ) -> wgpu::TextureView {
        if let Some(e) = self.map.get(name) {
            return e.view.clone();
        }
        let e = match frames::get(name) {
            Some(f) => {
                let texture = create(device, name, f.width, f.height);
                write(queue, &texture, f.width, f.height, &f.pixels);
                Entry {
                    view: texture.create_view(&Default::default()),
                    texture,
                    version: f.version,
                    size: (f.width, f.height),
                }
            }
            None => {
                let texture = create(device, name, 1, 1);
                write(queue, &texture, 1, 1, &[0, 0, 0, 255]);
                Entry {
                    view: texture.create_view(&Default::default()),
                    texture,
                    version: 0,
                    size: (1, 1),
                }
            }
        };
        let view = e.view.clone();
        self.map.insert(name.to_owned(), e);
        view
    }

    /// Upload frames published since the last call. True when a texture had to be recreated
    /// (new size), so bind groups holding its old view must be rebuilt.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
        let mut recreated = false;
        self.map.retain(|name, e| {
            let version = frames::version(name);
            if version == e.version {
                return true;
            }
            let Some(f) = frames::get(name) else {
                // Unpublished (the player stopped): forget it.
                recreated = true;
                return false;
            };
            if (f.width, f.height) != e.size {
                e.texture = create(device, name, f.width, f.height);
                e.view = e.texture.create_view(&Default::default());
                e.size = (f.width, f.height);
                recreated = true;
            }
            write(queue, &e.texture, f.width, f.height, &f.pixels);
            e.version = f.version;
            true
        });
        recreated
    }
}

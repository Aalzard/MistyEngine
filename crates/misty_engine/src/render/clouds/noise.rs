//! Cloud noise textures, made once when clouds are first drawn: the tiling 3D base-shape and
//! detail noises and the 2D weather map on the GPU (cloud_noise.wgsl), and a 64² blue-noise
//! tile on the CPU (void and cluster) for the per-pixel ray offsets.

const NOISE_SRC: &str = include_str!("shaders/cloud_noise.wgsl");
pub(super) const BASE_SIZE: u32 = 128;
pub(super) const DETAIL_SIZE: u32 = 32;
pub(super) const WEATHER_SIZE: u32 = 512;
pub(super) const BLUE_SIZE: usize = 64;

pub(super) struct NoiseTextures {
    pub base: wgpu::TextureView,
    pub detail: wgpu::TextureView,
    pub weather: wgpu::TextureView,
    pub blue: wgpu::TextureView,
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 3],
    dimension: wgpu::TextureDimension,
    format: wgpu::TextureFormat,
    storage: bool,
) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING;
    if storage {
        usage |= wgpu::TextureUsages::STORAGE_BINDING;
    } else {
        usage |= wgpu::TextureUsages::COPY_DST;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: size[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension,
        format,
        usage,
        view_formats: &[],
    })
}

impl NoiseTextures {
    /// Generate every texture (one GPU submit).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        use wgpu::TextureDimension as D;
        let rgba = wgpu::TextureFormat::Rgba8Unorm;
        let cube = |n| [n, n, n];
        let base = texture(
            device,
            "misty cloud base",
            cube(BASE_SIZE),
            D::D3,
            rgba,
            true,
        );
        let detail = texture(
            device,
            "misty cloud detail",
            cube(DETAIL_SIZE),
            D::D3,
            rgba,
            true,
        );
        let weather = texture(
            device,
            "misty cloud weather",
            [WEATHER_SIZE, WEATHER_SIZE, 1],
            D::D2,
            rgba,
            true,
        );
        let n = BLUE_SIZE as u32;
        let blue = texture(
            device,
            "misty blue noise",
            [n, n, 1],
            D::D2,
            wgpu::TextureFormat::R8Unorm,
            false,
        );
        queue.write_texture(
            blue.as_image_copy(),
            &blue_noise(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: 1,
            },
        );
        let views = [&base, &detail, &weather].map(|t| t.create_view(&Default::default()));
        generate(device, queue, &views);
        let [base, detail, weather] = views;
        Self {
            base,
            detail,
            weather,
            blue: blue.create_view(&Default::default()),
        }
    }

    pub fn bytes() -> u64 {
        let cube = |n: u32| 4 * (n as u64).pow(3);
        cube(BASE_SIZE)
            + cube(DETAIL_SIZE)
            + 4 * (WEATHER_SIZE as u64).pow(2)
            + (BLUE_SIZE * BLUE_SIZE) as u64
    }
}

/// Fill base, detail and weather with the three entry points of cloud_noise.wgsl.
fn generate(device: &wgpu::Device, queue: &wgpu::Queue, views: &[wgpu::TextureView; 3]) {
    let storage = |binding, dim| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: dim,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty cloud noise"),
        entries: &[
            storage(0, wgpu::TextureViewDimension::D3),
            storage(1, wgpu::TextureViewDimension::D2),
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty cloud noise"),
        source: wgpu::ShaderSource::Wgsl(NOISE_SRC.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty cloud noise"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let group = |volume: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty cloud noise"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(volume),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
            ],
        })
    };
    let jobs = [
        ("gen_base", &views[0], [BASE_SIZE.div_ceil(4); 3]),
        ("gen_detail", &views[1], [DETAIL_SIZE.div_ceil(4); 3]),
        (
            "gen_weather",
            &views[0],
            [WEATHER_SIZE.div_ceil(8), WEATHER_SIZE.div_ceil(8), 1],
        ),
    ];
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("misty cloud noise"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty cloud noise"),
            timestamp_writes: None,
        });
        for (entry, volume, [x, y, z]) in jobs {
            let p = pipeline(entry);
            pass.set_pipeline(&p);
            pass.set_bind_group(0, &group(volume), &[]);
            pass.dispatch_workgroups(x, y, z);
        }
    }
    queue.submit([encoder.finish()]);
}

/// 64² blue-noise ranks as bytes (void and cluster on a torus, Gaussian energy).
pub(super) fn blue_noise() -> Vec<u8> {
    static CACHE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| void_and_cluster(BLUE_SIZE)).clone()
}

/// Ranks 0..n² of a blue-noise dither array, scaled to bytes.
fn void_and_cluster(n: usize) -> Vec<u8> {
    let count = n * n;
    const SIGMA: f32 = 1.9;
    const R: i32 = 6;
    let kernel: Vec<f32> = (-R..=R)
        .flat_map(|y| {
            (-R..=R).map(move |x| (-((x * x + y * y) as f32) / (2.0 * SIGMA * SIGMA)).exp())
        })
        .collect();
    let mut energy = vec![0.0f32; count];
    let mut on = vec![false; count];
    let splat = |energy: &mut [f32], i: usize, sign: f32| {
        let (cx, cy) = ((i % n) as i32, (i / n) as i32);
        for dy in -R..=R {
            for dx in -R..=R {
                let x = (cx + dx).rem_euclid(n as i32) as usize;
                let y = (cy + dy).rem_euclid(n as i32) as usize;
                energy[y * n + x] += sign * kernel[((dy + R) * (2 * R + 1) + dx + R) as usize];
            }
        }
    };
    // Tightest cluster (highest energy among set points) / largest void (lowest among unset).
    let extreme = |energy: &[f32], on: &[bool], set: bool| -> usize {
        let mut best: Option<usize> = None;
        for i in (0..energy.len()).filter(|i| on[*i] == set) {
            let better = best.is_none_or(|b| match set {
                true => energy[i] > energy[b],
                false => energy[i] < energy[b],
            });
            if better {
                best = Some(i);
            }
        }
        best.unwrap_or(0)
    };
    // Initial pattern: 10% of the points from a hash, then relaxed until stable.
    let mut state = 0x9E37_79B9u32;
    let mut placed = 0;
    while placed < count / 10 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let i = state as usize % count;
        if !on[i] {
            on[i] = true;
            splat(&mut energy, i, 1.0);
            placed += 1;
        }
    }
    for _ in 0..count {
        let cluster = extreme(&energy, &on, true);
        on[cluster] = false;
        splat(&mut energy, cluster, -1.0);
        let void = extreme(&energy, &on, false);
        on[void] = true;
        splat(&mut energy, void, 1.0);
        if void == cluster {
            break;
        }
    }
    let mut rank = vec![0usize; count];
    // Phase 1: remove the initial points, tightest cluster first (ranks below `placed`).
    let (mut e1, mut on1) = (energy.clone(), on.clone());
    for r in (0..placed).rev() {
        let i = extreme(&e1, &on1, true);
        on1[i] = false;
        splat(&mut e1, i, -1.0);
        rank[i] = r;
    }
    // Phase 2: fill the largest voids (ranks from `placed` up).
    for r in placed..count {
        let i = extreme(&energy, &on, false);
        on[i] = true;
        splat(&mut energy, i, 1.0);
        rank[i] = r;
    }
    rank.iter()
        .map(|r| ((*r as f32 + 0.5) / count as f32 * 256.0) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_noise_is_a_permutation_with_high_frequency_content() {
        let b = blue_noise();
        assert_eq!(b.len(), BLUE_SIZE * BLUE_SIZE);
        // Every byte value appears equally often (ranks are a permutation).
        let mut hist = [0u32; 256];
        for v in &b {
            hist[*v as usize] += 1;
        }
        assert!(hist.iter().all(|h| *h == 16), "{hist:?}");
        // Neighbours differ much more than in white noise: mean |Δ| of a uniform 0..255 pair
        // is about 85; blue noise pushes neighbours apart.
        let n = BLUE_SIZE;
        let mut sum = 0u32;
        for y in 0..n {
            for x in 0..n {
                let a = b[y * n + x] as i32;
                let c = b[y * n + (x + 1) % n] as i32;
                sum += (a - c).unsigned_abs();
            }
        }
        let mean = sum as f32 / (n * n) as f32;
        assert!(mean > 90.0, "mean neighbour difference {mean}");
    }
}

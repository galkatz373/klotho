//! wgpu clustered-mesh presenter. Header-validate before upload.

use std::collections::BTreeMap;

use klotho_compile::decode_mesh;
use klotho_core::BlobId;
use klotho_manifest::{GpuBudget, LightKind, Observer, VisualManifest};
use klotho_prove::Cas;
use wgpu::util::DeviceExt;

use crate::math::{model_from_pose, view_proj};
use crate::palette::albedo;
use crate::presenter::{Presenter, draw_list};

const TARGET: u32 = 64;
const SHADER: &str = include_str!("shader.wgsl");

struct GpuMesh {
    verts: wgpu::Buffer,
    indices: wgpu::Buffer,
    nidx: u32,
}

/// Headless / windowed presenter. Owns GPU resources; borrows the manifest.
pub struct WgpuPresenter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    frame_bg: wgpu::BindGroup,
    frame_buf: wgpu::Buffer,
    object_layout: wgpu::BindGroupLayout,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    meshes: BTreeMap<BlobId, GpuMesh>,
    /// Last present: how many clusters were drawn.
    pub last_drawn: u16,
    /// Last present: how many blobs were rejected at header check.
    pub last_rejected: u16,
}

impl WgpuPresenter {
    /// Try to create a headless (no window) presenter. `None` if no adapter.
    pub fn try_headless() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })) {
                Ok(a) => a,
                Err(_) => {
                    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::LowPower,
                        compatible_surface: None,
                        force_fallback_adapter: true,
                    }))
                    .ok()?
                }
            };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("klotho-render"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        Some(Self::from_device(device, queue))
    }

    fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clustered-forward"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let object_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("object"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clustered"),
            bind_group_layouts: &[&frame_layout, &object_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("unlit-lambert"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame-ub"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame-bg"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });
        let color = make_target(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        let depth = make_target(&device, wgpu::TextureFormat::Depth24Plus);
        Self {
            device,
            queue,
            pipeline,
            frame_bg,
            frame_buf,
            object_layout,
            color,
            depth,
            meshes: BTreeMap::new(),
            last_drawn: 0,
            last_rejected: 0,
        }
    }

    /// Upload a mesh if the `KLTH` header validates. Returns whether it is cached.
    pub fn upload(&mut self, id: BlobId, bytes: &[u8]) -> bool {
        if self.meshes.contains_key(&id) {
            return true;
        }
        let Ok(decoded) = decode_mesh(bytes) else {
            return false;
        };
        let mut vbytes = Vec::with_capacity(decoded.verts.len() * 12);
        for v in &decoded.verts {
            for c in v {
                vbytes.extend_from_slice(&((*c as f32) / 1000.0).to_le_bytes());
            }
        }
        let ibytes: Vec<u8> = decoded
            .indices
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        let verts = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cluster-verts"),
                contents: &vbytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cluster-idx"),
                contents: &ibytes,
                usage: wgpu::BufferUsages::INDEX,
            });
        self.meshes.insert(
            id,
            GpuMesh {
                verts,
                indices,
                nidx: decoded.info.indices,
            },
        );
        true
    }

    /// Header-validate every cluster blob in `cas` that is not yet cached.
    pub fn upload_cas(&mut self, vis: &VisualManifest, cas: &Cas) {
        self.last_rejected = 0;
        for c in &vis.clusters {
            if self.meshes.contains_key(&c.blob) {
                continue;
            }
            let Some(bytes) = cas.get(c.blob) else {
                self.last_rejected = self.last_rejected.saturating_add(1);
                continue;
            };
            if !self.upload(c.blob, bytes) {
                self.last_rejected = self.last_rejected.saturating_add(1);
            }
        }
    }

    fn write_frame(&self, observer: Observer, vis: &VisualManifest) {
        let vp = view_proj(observer, 1.0);
        let light = light_dir(vis);
        let mut bytes = [0u8; 80];
        for (i, v) in vp.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        bytes[64..68].copy_from_slice(&light[0].to_le_bytes());
        bytes[68..72].copy_from_slice(&light[1].to_le_bytes());
        bytes[72..76].copy_from_slice(&light[2].to_le_bytes());
        self.queue.write_buffer(&self.frame_buf, 0, &bytes);
    }
}

fn light_dir(vis: &VisualManifest) -> [f32; 3] {
    for l in &vis.lights {
        if let LightKind::Point { .. } = l.kind {
            let x = l.pos.x as f32;
            let y = l.pos.y as f32;
            let z = l.pos.z as f32;
            let n = (x * x + y * y + z * z).sqrt().max(1.0);
            return [x / n, y / n, z / n];
        }
    }
    [0.35, 0.8, 0.45]
}

fn make_target(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::TextureView {
    let usage = if format == wgpu::TextureFormat::Depth24Plus {
        wgpu::TextureUsages::RENDER_ATTACHMENT
    } else {
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
    };
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: TARGET,
            height: TARGET,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    t.create_view(&wgpu::TextureViewDescriptor::default())
}

impl Presenter for WgpuPresenter {
    fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget) {
        let drawn = draw_list(vis, observer, budget);
        self.last_drawn = drawn.len() as u16;
        self.write_frame(observer, vis);
        let mut prepared: Vec<(BlobId, wgpu::BindGroup, wgpu::Buffer)> = Vec::new();
        for i in &drawn {
            let cluster = vis.clusters[*i];
            if !self.meshes.contains_key(&cluster.blob) {
                continue;
            }
            let mat = vis
                .materials
                .get(*i)
                .copied()
                .unwrap_or(klotho_manifest::MaterialRef {
                    tag: klotho_manifest::MaterialTag::Stone,
                    palette: 0,
                });
            let model = model_from_pose(cluster.pose);
            let alb = albedo(mat);
            let mut ub = [0u8; 96];
            for (k, v) in model.iter().enumerate() {
                ub[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            for (k, v) in alb.iter().enumerate() {
                ub[64 + k * 4..64 + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            ub[80..84].copy_from_slice(&(mat.tag as u32).to_le_bytes());
            let buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("object-ub"),
                    contents: &ub,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.object_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buf.as_entire_binding(),
                }],
            });
            prepared.push((cluster.blob, bg, buf));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clustered-forward"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bg, &[]);
            for (blob, bg, _) in &prepared {
                let Some(mesh) = self.meshes.get(blob) else {
                    continue;
                };
                pass.set_bind_group(1, bg, &[]);
                pass.set_vertex_buffer(0, mesh.verts.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.nidx, 0, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use klotho_compile::{Kitbash, cook_doc};
    use klotho_core::{Epoch, Hash, Mm, PoseMm, YawMd};
    use klotho_ir::{IntentDoc, Name, ProvenanceId, StyleIntent};
    use klotho_manifest::{GpuBudget, MaterialRef, Observer, VisualManifest};

    use super::*;

    fn skip_if_no_gpu() -> Option<WgpuPresenter> {
        WgpuPresenter::try_headless()
    }

    #[test]
    fn garbage_bytes_are_not_uploaded() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        assert!(!p.upload(BlobId::ZERO, b"XXXX"));
        assert!(p.meshes.is_empty());
    }

    #[test]
    fn kitbash_mesh_uploads_after_header_ok() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let doc = IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("door.oak.lockable")],
            },
            canon_diffs: Vec::new(),
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        };
        let cooked = cook_doc(&doc).unwrap();
        let tag = Kitbash::load_default().unwrap();
        let entry = tag.get("door.oak.lockable").unwrap();
        // The library encodes every catalog mesh; pick any hull/mesh from CAS
        // via the door bind table even without a seed locus.
        let mesh_id = cooked
            .bindings
            .iter()
            .find(|b| b.tag.as_str() == "door.oak.lockable")
            .map(|b| b.mesh)
            .or_else(|| {
                cooked.cas.iter().find_map(|(id, bytes)| {
                    if klotho_compile::validate_mesh(bytes).is_ok() {
                        Some(id)
                    } else {
                        None
                    }
                })
            })
            .unwrap();
        let bytes = cooked.cas.get(mesh_id).unwrap();
        assert!(klotho_compile::validate_mesh(bytes).is_ok());
        assert!(p.upload(mesh_id, bytes));
        let vis = VisualManifest::from_instances(
            Epoch::ZERO,
            [(
                mesh_id,
                PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
                MaterialRef {
                    tag: entry.material,
                    palette: 0,
                },
            )],
            [],
            [],
        );
        p.present(&vis, Observer::origin(), GpuBudget::HEARTH);
        assert_eq!(p.last_drawn, 1);
    }
}

//! wgpu clustered-mesh presenter. Header-validate before upload.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use klotho_compile::{decode_mesh, decode_skinned_mesh, peek_kind};
use klotho_core::BlobId;
use klotho_manifest::{GpuBudget, LightKind, Observer, VisualManifest};
use klotho_prove::{ArtifactKind, Cas};
use wgpu::util::DeviceExt;

use crate::math::{model_from_pose, view_proj};
use crate::palette::albedo;
use crate::pbr_pass::{self, PbrResources};
use crate::perm::{PresenterPerm, present_plan};
use crate::presenter::{Presenter, draw_list};

/// Offscreen golden size (16:9, row-aligned for readback).
pub const GOLDEN_WIDTH: u32 = 640;
/// Offscreen golden height.
pub const GOLDEN_HEIGHT: u32 = 360;
const SHADER: &str = include_str!("shader.wgsl");

pub(crate) struct GpuMesh {
    pub verts: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub nidx: u32,
}

/// Headless / windowed presenter. Owns GPU resources; borrows the manifest.
pub struct WgpuPresenter {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    frame_bg: wgpu::BindGroup,
    frame_buf: wgpu::Buffer,
    object_layout: wgpu::BindGroupLayout,
    color_tex: Option<wgpu::Texture>,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) meshes: BTreeMap<BlobId, GpuMesh>,
    pub(crate) skinned_meshes: BTreeMap<BlobId, GpuMesh>,
    /// Last present: how many clusters were drawn.
    pub last_drawn: u16,
    /// Last present: how many blobs were rejected at header check.
    pub last_rejected: u16,
    /// Encode + GPU-wait wall time of the last [`Self::present_to`], microseconds.
    ///
    /// After `submit` the presenter `poll(Wait)`s so this is not CPU encode
    /// alone. It is not a timestamp query (`TIMESTAMP_QUERY` stays off on
    /// downlevel). Fail-open compares this to [`GpuBudget::us_present`].
    pub last_present_us: u32,
    /// Cascades rendered on the last present (0 on the unlit path).
    pub last_cascades: u8,
    /// Composite `post.flags` packed into the full-res uniform (0 on unlit).
    pub last_post_flags: u32,
    pub(crate) probe_present: BTreeSet<BlobId>,
    pub(crate) pbr: Option<PbrResources>,
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
        Some(Self::from_device(
            device,
            queue,
            GOLDEN_WIDTH,
            GOLDEN_HEIGHT,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            true,
        ))
    }

    /// GPU device (surface configure).
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Build around an existing device. `offscreen` allocates a readback target.
    #[must_use]
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        offscreen: bool,
    ) -> Self {
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
                    format,
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
        let (color_tex, color) =
            make_color(&device, width.max(1), height.max(1), format, offscreen);
        let depth = make_depth(&device, width.max(1), height.max(1));
        Self {
            device,
            queue,
            pipeline,
            frame_bg,
            frame_buf,
            object_layout,
            color_tex,
            color,
            depth,
            width: width.max(1),
            height: height.max(1),
            format,
            meshes: BTreeMap::new(),
            skinned_meshes: BTreeMap::new(),
            last_drawn: 0,
            last_rejected: 0,
            last_present_us: 0,
            last_cascades: 0,
            last_post_flags: 0,
            probe_present: BTreeSet::new(),
            pbr: None,
        }
    }

    /// Pixel size of the current target.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Upload a mesh if the `KLTH` header validates. Returns whether it is cached.
    pub fn upload(&mut self, id: BlobId, bytes: &[u8]) -> bool {
        if self.meshes.contains_key(&id) || self.skinned_meshes.contains_key(&id) {
            return true;
        }
        match peek_kind(bytes) {
            Ok(ArtifactKind::ClusteredMesh) => self.upload_clustered(id, bytes),
            Ok(ArtifactKind::SkinnedMesh) => self.upload_skinned(id, bytes),
            _ => false,
        }
    }

    fn upload_clustered(&mut self, id: BlobId, bytes: &[u8]) -> bool {
        let Ok(decoded) = decode_mesh(bytes) else {
            return false;
        };
        let mut vbytes = Vec::with_capacity(decoded.verts.len() * 12);
        for v in &decoded.verts {
            for c in v {
                vbytes.extend_from_slice(&((*c as f32) / 1000.0).to_le_bytes());
            }
        }
        let mesh = self.gpu_mesh("cluster-verts", "cluster-idx", &vbytes, &decoded.indices);
        self.meshes.insert(id, mesh);
        true
    }

    fn upload_skinned(&mut self, id: BlobId, bytes: &[u8]) -> bool {
        let Ok(decoded) = decode_skinned_mesh(bytes) else {
            return false;
        };
        let mut vbytes = Vec::with_capacity(decoded.verts.len() * 48);
        for i in 0..decoded.verts.len() {
            for c in decoded.verts[i] {
                vbytes.extend_from_slice(&((c as f32) / 1000.0).to_le_bytes());
            }
            vbytes.extend_from_slice(&0f32.to_le_bytes());
            for c in decoded.joints[i] {
                vbytes.extend_from_slice(&(f32::from(c)).to_le_bytes());
            }
            for c in decoded.weights[i] {
                vbytes.extend_from_slice(&(f32::from(c) / 65_535.0).to_le_bytes());
            }
        }
        let mesh = self.gpu_mesh("skin-verts", "skin-idx", &vbytes, &decoded.indices);
        self.skinned_meshes.insert(id, mesh);
        true
    }

    fn gpu_mesh(&self, vlabel: &str, ilabel: &str, vbytes: &[u8], indices: &[u32]) -> GpuMesh {
        let ibytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let verts = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(vlabel),
                contents: vbytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(ilabel),
                contents: &ibytes,
                usage: wgpu::BufferUsages::INDEX,
            });
        GpuMesh {
            verts,
            indices: indices_buf,
            nidx: indices.len() as u32,
        }
    }

    /// Header-validate every cluster blob in `cas` that is not yet cached.
    pub fn upload_cas(&mut self, vis: &VisualManifest, cas: &Cas) {
        self.last_rejected = 0;
        self.probe_present.clear();
        for g in &vis.probes {
            if cas.get(g.blob).is_some() {
                self.probe_present.insert(g.blob);
            }
        }
        for blob in vis
            .clusters
            .iter()
            .map(|c| c.blob)
            .chain(vis.masked.iter().map(|c| c.blob))
            .chain(vis.skinned.iter().map(|c| c.blob))
        {
            if self.meshes.contains_key(&blob) || self.skinned_meshes.contains_key(&blob) {
                continue;
            }
            let Some(bytes) = cas.get(blob) else {
                self.last_rejected = self.last_rejected.saturating_add(1);
                continue;
            };
            if !self.upload(blob, bytes) {
                self.last_rejected = self.last_rejected.saturating_add(1);
            }
        }
    }

    /// Draw into an external color view (swapchain). Depth must match [`Self::size`].
    pub fn present_to(
        &mut self,
        color: &wgpu::TextureView,
        vis: &VisualManifest,
        observer: Observer,
        budget: GpuBudget,
    ) {
        let start = Instant::now();
        let plan = present_plan(vis.post, self.last_present_us, budget);
        match plan.perm {
            PresenterPerm::Unlit => {
                self.last_cascades = 0;
                self.last_post_flags = 0;
                self.present_unlit(color, vis, observer, budget);
            }
            _ => pbr_pass::present_pbr(self, color, vis, observer, budget, plan),
        }
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        let us = start.elapsed().as_micros();
        self.last_present_us = u32::try_from(us).unwrap_or(u32::MAX);
    }

    fn present_unlit(
        &mut self,
        color: &wgpu::TextureView,
        vis: &VisualManifest,
        observer: Observer,
        budget: GpuBudget,
    ) {
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
                    view: color,
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

    /// Resize depth (and offscreen color if present) to a new swapchain size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        let (tex, view) = make_color(
            &self.device,
            width,
            height,
            self.format,
            self.color_tex.is_some(),
        );
        self.color_tex = tex;
        self.color = view;
        self.depth = make_depth(&self.device, width, height);
        if let Some(pbr) = self.pbr.as_mut() {
            pbr.resize(&self.device, self.format, width, height);
        }
    }

    /// Read the offscreen target. `None` if this presenter has no readback texture.
    pub fn read_rgba(&self) -> Option<Vec<u8>> {
        let tex = self.color_tex.as_ref()?;
        let bpp = 4u32;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded = self.width * bpp;
        let padded = unpadded.div_ceil(align) * align;
        let size = u64::from(padded) * u64::from(self.height);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        encoder.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        let data = slice.get_mapped_range();
        let mut out = vec![0u8; (self.width * self.height * 4) as usize];
        for y in 0..self.height as usize {
            let src = y * padded as usize;
            let dst = y * unpadded as usize;
            out[dst..dst + unpadded as usize].copy_from_slice(&data[src..src + unpadded as usize]);
        }
        drop(data);
        buf.unmap();
        Some(out)
    }

    fn write_frame(&self, observer: Observer, vis: &VisualManifest) {
        let aspect = self.width as f32 / self.height.max(1) as f32;
        let vp = view_proj(observer, aspect);
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

fn make_color(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    offscreen: bool,
) -> (Option<wgpu::Texture>, wgpu::TextureView) {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = t.create_view(&wgpu::TextureViewDescriptor::default());
    if offscreen {
        (Some(t), view)
    } else {
        (None, view)
    }
}

fn make_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    t.create_view(&wgpu::TextureViewDescriptor::default())
}

impl Presenter for WgpuPresenter {
    fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget) {
        let view = self.color.clone();
        self.present_to(&view, vis, observer, budget);
    }
}

#[cfg(test)]
mod tests {
    use klotho_compile::{Kitbash, cook_doc};
    use klotho_core::{BlobId, Epoch, Hash, IVec3, Mm, PoseMm, YawMd};
    use klotho_ir::{IntentDoc, Name, ProvenanceId, StyleIntent};
    use klotho_manifest::{
        GpuBudget, LightKind, LightStub, MaterialRef, Observer, PostFlags, ProbeGrid,
        VisualManifest,
    };

    use super::*;

    fn skip_if_no_gpu() -> Option<WgpuPresenter> {
        WgpuPresenter::try_headless()
    }

    fn door_mesh() -> (BlobId, Vec<u8>, MaterialRef) {
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
        let bytes = cooked.cas.get(mesh_id).unwrap().to_vec();
        (
            mesh_id,
            bytes,
            MaterialRef {
                tag: entry.material,
                palette: 0,
            },
        )
    }

    fn vis_with(
        mesh_id: BlobId,
        mat: MaterialRef,
        post: PostFlags,
        lights: Vec<LightStub>,
    ) -> VisualManifest {
        let mut vis = VisualManifest::from_instances(
            Epoch::ZERO,
            [(mesh_id, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO), mat)],
            lights,
            [],
        );
        vis.post = post;
        vis
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
        let (mesh_id, bytes, mat) = door_mesh();
        assert!(klotho_compile::validate_mesh(&bytes).is_ok());
        assert!(p.upload(mesh_id, &bytes));
        let vis = vis_with(mesh_id, mat, PostFlags::UNLIT, vec![]);
        p.present(&vis, Observer::origin(), GpuBudget::HEARTH);
        assert_eq!(p.last_drawn, 1);
        assert_eq!(p.last_cascades, 0);
        assert_eq!(p.last_post_flags, 0);
        assert!(p.pbr.is_none());
    }

    #[test]
    fn adventure_present_draws_one() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let (mesh_id, bytes, mat) = door_mesh();
        assert!(p.upload(mesh_id, &bytes));
        let vis = vis_with(mesh_id, mat, PostFlags::ADVENTURE, vec![]);
        p.present(&vis, Observer::origin(), GpuBudget::AAA_ADVENTURE);
        assert_eq!(p.last_drawn, 1);
        assert_eq!(p.last_cascades, 3);
        assert!(p.pbr.is_some());
        assert_ne!(p.last_post_flags & 1, 0);
        assert_ne!(p.last_post_flags & 2, 0);
    }

    #[test]
    fn competitive_present_with_sun_draws_one() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let (mesh_id, bytes, mat) = door_mesh();
        assert!(p.upload(mesh_id, &bytes));
        let sun = LightStub {
            pos: IVec3 {
                x: 0,
                y: 4000,
                z: 0,
            },
            kind: LightKind::Sun {
                dir: IVec3 {
                    x: 350,
                    y: 800,
                    z: 450,
                },
                intensity_milli: 1000,
            },
        };
        let vis = vis_with(mesh_id, mat, PostFlags::COMPETITIVE, vec![sun]);
        p.present(&vis, Observer::origin(), GpuBudget::AAA_SHOOTER);
        assert_eq!(p.last_drawn, 1);
        assert_eq!(p.last_cascades, 1);
        assert_eq!(p.last_post_flags, 0);
    }

    #[test]
    fn missing_probe_blob_does_not_panic() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let (mesh_id, bytes, mat) = door_mesh();
        assert!(p.upload(mesh_id, &bytes));
        let mut vis = vis_with(mesh_id, mat, PostFlags::ADVENTURE, vec![]);
        vis.probes.push(ProbeGrid {
            blob: BlobId::from_bytes([9; 32]),
            origin: IVec3 { x: 0, y: 0, z: 0 },
            spacing_mm: 2000,
            dim: (4, 2, 4),
        });
        p.present(&vis, Observer::origin(), GpuBudget::AAA_ADVENTURE);
        assert_eq!(p.last_drawn, 1);
        assert_eq!(p.last_cascades, 3);
    }

    fn tri_verts() -> [[i16; 3]; 3] {
        [[-2000, 0, 3000], [2000, 0, 3000], [0, 3200, 3000]]
    }

    fn clustered_tri() -> Vec<u8> {
        let verts = tri_verts();
        let mut b = Vec::new();
        b.extend_from_slice(b"KLTH");
        b.push(1);
        b.push(0);
        b.push(0);
        b.push(0);
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes());
        for v in verts {
            for c in v {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in [0u32, 2, 1] {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b
    }

    fn skinned_tri(bones: u32, joints: [u8; 4], weights: [u16; 4]) -> Vec<u8> {
        let verts = tri_verts();
        klotho_compile::encode_skinned_mesh(
            &verts,
            &[joints, joints, joints],
            &[weights, weights, weights],
            &[0, 2, 1],
            bones,
        )
        .unwrap()
    }

    fn skinned_vis(
        blob: BlobId,
        pose: PoseMm,
        slot: klotho_manifest::PaletteSlot,
        post: PostFlags,
    ) -> VisualManifest {
        let mat = MaterialRef {
            tag: klotho_manifest::MaterialTag::Stone,
            palette: 0,
        };
        VisualManifest::from_v2(
            Epoch::ZERO,
            klotho_core::Tick::ZERO,
            [],
            [],
            [klotho_manifest::SkinnedInstance {
                blob,
                gpu: klotho_manifest::GpuHandle::NONE,
                pose,
                palette: 0,
                material: mat,
            }],
            [slot],
            [],
            [],
            post,
            [],
        )
    }

    #[test]
    fn invalid_skinned_mesh_is_not_uploaded() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        assert!(!p.upload(BlobId::from_bytes([1; 32]), b"XXXX"));
        assert!(p.skinned_meshes.is_empty());
        let mut bad = skinned_tri(1, [0; 4], [klotho_compile::SKIN_WEIGHT_SUM as u16, 0, 0, 0]);
        bad[5] = 99;
        assert!(!p.upload(BlobId::from_bytes([2; 32]), &bad));
    }

    #[test]
    fn unlit_skinned_does_not_panic() {
        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let id = BlobId::from_bytes([3; 32]);
        let bytes = skinned_tri(0, [0; 4], [klotho_compile::SKIN_WEIGHT_SUM as u16, 0, 0, 0]);
        assert!(p.upload(id, &bytes));
        let vis = skinned_vis(
            id,
            PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
            klotho_manifest::PaletteSlot::identity(),
            PostFlags::UNLIT,
        );
        p.present(&vis, Observer::origin(), GpuBudget::HEARTH);
        assert_eq!(p.last_drawn, 0);
        assert!(p.pbr.is_none());
    }

    #[test]
    fn identity_palette_matches_rigid_and_clip_moves() {
        let pos = klotho_core::IVec3 {
            x: -2000,
            y: 0,
            z: 3000,
        };
        let root = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let w = [klotho_compile::SKIN_WEIGHT_SUM as u16, 0, 0, 0];
        let identity = klotho_anim::skin_world(pos, root, &[], [0; 4], w);
        let rigid = klotho_anim::apply_pose(root, pos);
        assert_eq!(identity, rigid);

        let joint = PoseMm::new(Mm(2500), Mm(0), Mm(0), YawMd::ZERO);
        let moved = klotho_anim::skin_world(pos, root, &[joint], [0; 4], w);
        assert_ne!(moved, identity);

        let Some(mut p) = skip_if_no_gpu() else {
            return;
        };
        let rigid_id = BlobId::from_bytes([10; 32]);
        let skin_id = BlobId::from_bytes([11; 32]);
        let tpose_id = BlobId::from_bytes([12; 32]);
        assert!(p.upload(rigid_id, &clustered_tri()));
        let skin_bytes = skinned_tri(1, [0; 4], w);
        assert!(p.upload(skin_id, &skin_bytes));
        assert!(p.upload(tpose_id, &skin_bytes));

        let mat = MaterialRef {
            tag: klotho_manifest::MaterialTag::Stone,
            palette: 0,
        };
        let rigid_vis = vis_with(rigid_id, mat, PostFlags::COMPETITIVE, vec![]);
        p.present(&rigid_vis, Observer::origin(), GpuBudget::AAA_SHOOTER);
        let rigid_px = p.read_rgba().expect("readback");
        assert!(
            rigid_px.chunks_exact(4).any(|c| c != [63, 63, 75, 255]),
            "rigid triangle was not drawn"
        );

        let ident_vis = skinned_vis(
            skin_id,
            root,
            klotho_manifest::PaletteSlot::identity(),
            PostFlags::COMPETITIVE,
        );
        p.present(&ident_vis, Observer::origin(), GpuBudget::AAA_SHOOTER);
        assert_eq!(p.last_drawn, 1);
        let ident_px = p.read_rgba().expect("readback");
        assert!(
            ident_px.chunks_exact(4).any(|c| c != [63, 63, 75, 255]),
            "identity skinned triangle was not drawn"
        );
        assert_eq!(ident_px, rigid_px);

        let moved_vis = skinned_vis(
            tpose_id,
            root,
            klotho_manifest::PaletteSlot {
                gpu: klotho_manifest::GpuHandle::NONE,
                bones: 1,
                joints: vec![joint],
            },
            PostFlags::COMPETITIVE,
        );
        p.present(&moved_vis, Observer::origin(), GpuBudget::AAA_SHOOTER);
        assert_eq!(p.last_drawn, 1);
        let moved_px = p.read_rgba().expect("readback");
        assert_ne!(moved_px, ident_px);
    }
}

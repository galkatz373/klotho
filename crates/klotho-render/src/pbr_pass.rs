//! Forward+ PBR pipelines. Built on first non-unlit present so goldens stay on lambert.

use klotho_core::BlobId;
use klotho_manifest::{GpuBudget, LightKind, MaterialRef, MaterialTag, Observer, VisualManifest};
use wgpu::util::DeviceExt;

use crate::cluster::{
    MAX_POINT_LIGHTS, MAX_TILES_X, MAX_TILES_Y, PointLight, TileAssign, assign_tiles,
    collect_point_lights,
};
use crate::gpu::WgpuPresenter;
use crate::math::{cascade_view_projs, eye_metres, model_from_pose, view_proj};
use crate::palette::{albedo, metalness_roughness};
use crate::perm::PresentPlan;
use crate::presenter::draw_list;
use crate::probes::probe_grid_usable;

const PBR_SRC: &str = include_str!("pbr.wgsl");
const POST_SRC: &str = include_str!("post.wgsl");
const FRAME_BYTES: u64 = 368;
const LIGHTS_BYTES: u64 = (MAX_POINT_LIGHTS * 32) as u64;
const TILES_BYTES: u64 = (MAX_TILES_X * MAX_TILES_Y * 4) as u64;
const POST_BYTES: u64 = 16;
const SHADOW_SIZE: u32 = 1024;
const SHADOW_LAYERS: u32 = 3;
const PROBE_DIM: u32 = 4;

pub(crate) struct PbrResources {
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    skinned_pipeline: wgpu::RenderPipeline,
    skinned_shadow_pipeline: wgpu::RenderPipeline,
    ssgi_pipeline: wgpu::RenderPipeline,
    bloom_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    object_layout: wgpu::BindGroupLayout,
    skinned_object_layout: wgpu::BindGroupLayout,
    shadow_frame_bg: wgpu::BindGroup,
    blit_layout: wgpu::BindGroupLayout,
    ssgi_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    frame_buf: wgpu::Buffer,
    lights_buf: wgpu::Buffer,
    tiles_buf: wgpu::Buffer,
    post_half_buf: wgpu::Buffer,
    post_full_buf: wgpu::Buffer,
    frame_bg: wgpu::BindGroup,
    shadow_layer_views: [wgpu::TextureView; 3],
    _shadow_tex: wgpu::Texture,
    linear_samp: wgpu::Sampler,
    nearest_samp: wgpu::Sampler,
    pbr_color: wgpu::Texture,
    pbr_color_view: wgpu::TextureView,
    pbr_depth: wgpu::Texture,
    pbr_depth_view: wgpu::TextureView,
    ssgi_tex: wgpu::Texture,
    ssgi_view: wgpu::TextureView,
    bloom_tex: wgpu::Texture,
    bloom_view: wgpu::TextureView,
    resolve_tex: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    history_tex: wgpu::Texture,
    history_view: wgpu::TextureView,
    _probe_tex: wgpu::Texture,
    history_valid: bool,
    width: u32,
    height: u32,
}

impl PbrResources {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let pbr_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pbr-forward"),
            source: wgpu::ShaderSource::Wgsl(PBR_SRC.into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pbr-post"),
            source: wgpu::ShaderSource::Wgsl(POST_SRC.into()),
        });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pbr-frame"),
            entries: &[
                ub_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                ub_entry(1, wgpu::ShaderStages::FRAGMENT),
                ub_entry(2, wgpu::ShaderStages::FRAGMENT),
                tex_entry(
                    3,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::TextureSampleType::Depth,
                    wgpu::TextureViewDimension::D2Array,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                tex_entry(
                    5,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::TextureSampleType::Float { filterable: true },
                    wgpu::TextureViewDimension::D3,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let object_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pbr-object"),
            entries: &[ub_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
        });
        let skinned_object_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("pbr-skinned-object"),
                entries: &[
                    ub_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                    ub_entry(1, wgpu::ShaderStages::VERTEX),
                ],
            });
        let blit_layout = post_layout(device, "pbr-blit-layout");
        let ssgi_layout = ssgi_bg_layout(device);
        let composite_layout = composite_bg_layout(device);

        let pbr_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr"),
            bind_group_layouts: &[&frame_layout, &object_layout],
            push_constant_ranges: &[],
        });
        let shadow_frame_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("pbr-shadow-frame"),
                entries: &[ub_entry(
                    0,
                    wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                )],
            });
        let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-shadow-pl"),
            bind_group_layouts: &[&shadow_frame_layout, &object_layout],
            push_constant_ranges: &[],
        });
        let skinned_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-skinned"),
            bind_group_layouts: &[&frame_layout, &skinned_object_layout],
            push_constant_ranges: &[],
        });
        let skinned_shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-skinned-shadow-pl"),
            bind_group_layouts: &[&shadow_frame_layout, &skinned_object_layout],
            push_constant_ranges: &[],
        });
        let pipeline = color_pipeline(
            device,
            &pbr_pl,
            &pbr_shader,
            "vs",
            "fs",
            format,
            true,
            "pbr-forward",
            vert_layout(),
        );
        let skinned_pipeline = color_pipeline(
            device,
            &skinned_pl,
            &pbr_shader,
            "vs_skinned",
            "fs",
            format,
            true,
            "pbr-forward-skinned",
            skinned_vert_layout(),
        );
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pbr-shadow"),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState {
                module: &pbr_shader,
                entry_point: Some("vs_shadow"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vert_layout()],
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let skinned_shadow_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("pbr-skinned-shadow"),
                layout: Some(&skinned_shadow_pl),
                vertex: wgpu::VertexState {
                    module: &pbr_shader,
                    entry_point: Some("vs_shadow_skinned"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[skinned_vert_layout()],
                },
                fragment: None,
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: Some(wgpu::Face::Front),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-blit-pl"),
            bind_group_layouts: &[&blit_layout],
            push_constant_ranges: &[],
        });
        let ssgi_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-ssgi-pl"),
            bind_group_layouts: &[&ssgi_layout],
            push_constant_ranges: &[],
        });
        let composite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pbr-composite-pl"),
            bind_group_layouts: &[&composite_layout],
            push_constant_ranges: &[],
        });
        let ssgi_pipeline = fullscreen_pipeline(
            device,
            &ssgi_pl,
            &post_shader,
            "fs_ssgi",
            format,
            "pbr-ssgi",
        );
        let bloom_pipeline = fullscreen_pipeline(
            device,
            &blit_pl,
            &post_shader,
            "fs_bloom",
            format,
            "pbr-bloom",
        );
        let composite_pipeline = fullscreen_pipeline(
            device,
            &composite_pl,
            &post_shader,
            "fs_composite",
            format,
            "pbr-composite",
        );
        let blit_pipeline = fullscreen_pipeline(
            device,
            &blit_pl,
            &post_shader,
            "fs_blit",
            format,
            "pbr-blit",
        );

        let frame_buf = ubuf(device, "pbr-frame-ub", FRAME_BYTES);
        let lights_buf = ubuf(device, "pbr-lights-ub", LIGHTS_BYTES);
        let tiles_buf = ubuf(device, "pbr-tiles-ub", TILES_BYTES);
        let post_half_buf = ubuf(device, "pbr-post-half-ub", POST_BYTES);
        let post_full_buf = ubuf(device, "pbr-post-full-ub", POST_BYTES);

        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pbr-shadow"),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: SHADOW_LAYERS,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("pbr-shadow-array"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: Some(SHADOW_LAYERS),
            ..Default::default()
        });
        let shadow_layer_views = [
            shadow_layer_view(&shadow_tex, 0),
            shadow_layer_view(&shadow_tex, 1),
            shadow_layer_view(&shadow_tex, 2),
        ];
        let shadow_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pbr-shadow-cmp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let (probe_tex, probe_view) = make_probe(device, queue);
        let probe_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pbr-probe"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let linear_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pbr-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let nearest_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pbr-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let shadow_frame_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pbr-shadow-frame-bg"),
            layout: &shadow_frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });
        let frame_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pbr-frame-bg"),
            layout: &frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lights_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: tiles_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&shadow_samp),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&probe_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&probe_samp),
                },
            ],
        });
        let width = width.max(1);
        let height = height.max(1);
        let hw = (width / 2).max(1);
        let hh = (height / 2).max(1);
        let pbr_color = color_target(device, "pbr-color", format, width, height);
        let pbr_color_view = pbr_color.create_view(&wgpu::TextureViewDescriptor::default());
        let pbr_depth = depth_target(device, width, height);
        let pbr_depth_view = pbr_depth.create_view(&wgpu::TextureViewDescriptor::default());
        let ssgi_tex = color_target(device, "pbr-ssgi", format, hw, hh);
        let ssgi_view = ssgi_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bloom_tex = color_target(device, "pbr-bloom", format, hw, hh);
        let bloom_view = bloom_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_tex = color_target(device, "pbr-resolve", format, width, height);
        let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let history_tex = color_target(device, "pbr-history", format, width, height);
        let history_view = history_tex.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            pipeline,
            shadow_pipeline,
            skinned_pipeline,
            skinned_shadow_pipeline,
            ssgi_pipeline,
            bloom_pipeline,
            composite_pipeline,
            blit_pipeline,
            object_layout,
            skinned_object_layout,
            shadow_frame_bg,
            blit_layout,
            ssgi_layout,
            composite_layout,
            frame_buf,
            lights_buf,
            tiles_buf,
            post_half_buf,
            post_full_buf,
            frame_bg,
            _shadow_tex: shadow_tex,
            shadow_layer_views,
            linear_samp,
            nearest_samp,
            pbr_color,
            pbr_color_view,
            pbr_depth,
            pbr_depth_view,
            ssgi_tex,
            ssgi_view,
            bloom_tex,
            bloom_view,
            resolve_tex,
            resolve_view,
            history_tex,
            history_view,
            _probe_tex: probe_tex,
            history_valid: false,
            width,
            height,
        }
    }

    pub(crate) fn resize(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        let hw = (width / 2).max(1);
        let hh = (height / 2).max(1);
        self.pbr_color = color_target(device, "pbr-color", format, width, height);
        self.pbr_color_view = self
            .pbr_color
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.pbr_depth = depth_target(device, width, height);
        self.pbr_depth_view = self
            .pbr_depth
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.ssgi_tex = color_target(device, "pbr-ssgi", format, hw, hh);
        self.ssgi_view = self
            .ssgi_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.bloom_tex = color_target(device, "pbr-bloom", format, hw, hh);
        self.bloom_view = self
            .bloom_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.resolve_tex = color_target(device, "pbr-resolve", format, width, height);
        self.resolve_view = self
            .resolve_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.history_tex = color_target(device, "pbr-history", format, width, height);
        self.history_view = self
            .history_tex
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.history_valid = false;
    }
}

pub(crate) fn present_pbr(
    gpu: &mut WgpuPresenter,
    color: &wgpu::TextureView,
    vis: &VisualManifest,
    observer: Observer,
    budget: GpuBudget,
    plan: PresentPlan,
) {
    if gpu.pbr.is_none() {
        gpu.pbr = Some(PbrResources::new(
            &gpu.device,
            &gpu.queue,
            gpu.format,
            gpu.width,
            gpu.height,
        ));
    }
    if let Some(pbr) = gpu.pbr.as_mut() {
        pbr.resize(&gpu.device, gpu.format, gpu.width, gpu.height);
    }

    let drawn = draw_list(vis, observer, budget);
    let items = pbr_draw_items(vis, &drawn);
    gpu.last_drawn = items.len() as u16;
    gpu.last_cascades = plan.cascades;

    let aspect = gpu.width as f32 / gpu.height.max(1) as f32;
    let vp = view_proj(observer, aspect);
    let (sun_dir, sun_i) = sun_dir_intensity(vis);
    let (cascades, splits) = cascade_view_projs(observer, aspect, sun_dir, plan.cascades);
    let points = collect_point_lights(vis);
    let tiles = assign_tiles(&points, &vp, gpu.width, gpu.height);
    let probe = if plan.gi {
        vis.probes
            .iter()
            .find(|g| gpu.probe_present.contains(&g.blob) && probe_grid_usable(g))
    } else {
        None
    };
    let eye = eye_metres(observer);
    let frame_bytes = pack_frame(
        &vp,
        &cascades,
        sun_dir,
        sun_i,
        eye,
        splits,
        &tiles,
        points.len() as u32,
        gpu.width,
        gpu.height,
        plan.cascades,
        probe,
    );
    let lights_bytes = pack_lights(&points);
    let tiles_bytes = pack_tiles(&tiles);

    let prepared = prepare_draws(gpu, vis, &items);
    let need_post = plan.ssgi || plan.bloom || plan.taa;
    let mut flags = 0u32;
    if plan.ssgi {
        flags |= 1;
    }
    if plan.bloom {
        flags |= 2;
    }
    if plan.taa && gpu.pbr.as_ref().is_some_and(|p| p.history_valid) {
        flags |= 4;
    }
    gpu.last_post_flags = flags;
    {
        let pbr = gpu.pbr.as_ref().expect("pbr");
        gpu.queue.write_buffer(&pbr.frame_buf, 0, &frame_bytes);
        gpu.queue.write_buffer(&pbr.lights_buf, 0, &lights_bytes);
        gpu.queue.write_buffer(&pbr.tiles_buf, 0, &tiles_bytes);
        write_post(
            &gpu.queue,
            &pbr.post_half_buf,
            (gpu.width / 2).max(1),
            (gpu.height / 2).max(1),
            0,
        );
        write_post(&gpu.queue, &pbr.post_full_buf, gpu.width, gpu.height, flags);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present-pbr"),
            });

        for layer in 0..plan.cascades {
            let view = &pbr.shadow_layer_views[layer as usize];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pbr-shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &pbr.shadow_frame_bg, &[]);
            let inst = u32::from(layer);
            draw_prepared(&mut pass, gpu, &prepared, inst..inst + 1, true);
        }

        let pbr_target = if need_post {
            &pbr.pbr_color_view
        } else {
            color
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pbr-forward"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: pbr_target,
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
                    view: &pbr.pbr_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &pbr.frame_bg, &[]);
            draw_prepared(&mut pass, gpu, &prepared, 0..1, false);
        }

        if plan.ssgi {
            let bg = ssgi_bg(gpu, pbr);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pbr-ssgi"),
                color_attachments: &[Some(load_store(&pbr.ssgi_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pbr.ssgi_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
        }
        if plan.bloom {
            let bg = blit_bg(gpu, pbr, &pbr.pbr_color_view, &pbr.post_half_buf);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pbr-bloom"),
                color_attachments: &[Some(load_store(&pbr.bloom_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pbr.bloom_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
        }
        if need_post {
            let bg = composite_bg(gpu, pbr);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pbr-composite"),
                    color_attachments: &[Some(load_store(&pbr.resolve_view))],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pbr.composite_pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.draw(0..3, 0..1);
            }
            let blit_bg = blit_bg(gpu, pbr, &pbr.resolve_view, &pbr.post_full_buf);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pbr-blit"),
                    color_attachments: &[Some(load_store(color))],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pbr.blit_pipeline);
                pass.set_bind_group(0, &blit_bg, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_texture(
                pbr.resolve_tex.as_image_copy(),
                pbr.history_tex.as_image_copy(),
                wgpu::Extent3d {
                    width: gpu.width,
                    height: gpu.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        gpu.queue.submit(Some(encoder.finish()));
    }
    if need_post {
        if let Some(pbr) = gpu.pbr.as_mut() {
            pbr.history_valid = true;
        }
    }
}

struct Prepared {
    blob: BlobId,
    bg: wgpu::BindGroup,
    _buf: wgpu::Buffer,
    _palette_buf: Option<wgpu::Buffer>,
    skinned: bool,
}

/// One PBR instance. `palette` is `Some` for skinned draws.
#[derive(Copy, Clone, Debug)]
pub(crate) struct PbrDrawItem {
    /// Mesh blob.
    pub blob: BlobId,
    /// Admitted root pose.
    pub pose: klotho_core::PoseMm,
    /// Closed material.
    pub mat: MaterialRef,
    /// Index into [`VisualManifest::palettes`]. `None` is rigid.
    pub palette: Option<u16>,
}

/// Opaque (`drawn`) + masked + skinned with a valid palette index.
pub(crate) fn pbr_draw_items(vis: &VisualManifest, drawn: &[usize]) -> Vec<PbrDrawItem> {
    let mut out = Vec::new();
    for i in drawn {
        let cluster = vis.clusters[*i];
        let mat = vis.materials.get(*i).copied().unwrap_or(MaterialRef {
            tag: MaterialTag::Stone,
            palette: 0,
        });
        out.push(PbrDrawItem {
            blob: cluster.blob,
            pose: cluster.pose,
            mat,
            palette: None,
        });
    }
    for (i, cluster) in vis.masked.iter().enumerate() {
        let mat = vis.masked_materials.get(i).copied().unwrap_or(MaterialRef {
            tag: MaterialTag::Stone,
            palette: 0,
        });
        out.push(PbrDrawItem {
            blob: cluster.blob,
            pose: cluster.pose,
            mat,
            palette: None,
        });
    }
    for inst in &vis.skinned {
        if vis.palettes.get(usize::from(inst.palette)).is_none() {
            continue;
        }
        out.push(PbrDrawItem {
            blob: inst.blob,
            pose: inst.pose,
            mat: inst.material,
            palette: Some(inst.palette),
        });
    }
    out
}

fn prepare_draws(
    gpu: &WgpuPresenter,
    vis: &VisualManifest,
    items: &[PbrDrawItem],
) -> Vec<Prepared> {
    let pbr = gpu.pbr.as_ref().expect("pbr");
    let mut prepared = Vec::new();
    for item in items {
        if let Some(ix) = item.palette {
            let Some(slot) = vis.palettes.get(usize::from(ix)) else {
                continue;
            };
            if gpu.skinned_meshes.contains_key(&item.blob) {
                prepared.push(make_skinned(gpu, pbr, item, slot));
            } else if gpu.meshes.contains_key(&item.blob) {
                prepared.push(make_prepared(
                    gpu, pbr, item.blob, item.pose, item.mat, false,
                ));
            }
        } else if gpu.meshes.contains_key(&item.blob) {
            prepared.push(make_prepared(
                gpu, pbr, item.blob, item.pose, item.mat, false,
            ));
        }
    }
    prepared
}

fn make_prepared(
    gpu: &WgpuPresenter,
    pbr: &PbrResources,
    blob: BlobId,
    pose: klotho_core::PoseMm,
    mat: MaterialRef,
    skinned: bool,
) -> Prepared {
    let model = model_from_pose(pose);
    let alb = albedo(mat);
    let (metalness, roughness) = metalness_roughness(mat.tag);
    let ub = pack_object(&model, alb, mat.tag as u32, metalness, roughness);
    let buf = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pbr-object-ub"),
            contents: &ub,
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pbr.object_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buf.as_entire_binding(),
        }],
    });
    Prepared {
        blob,
        bg,
        _buf: buf,
        _palette_buf: None,
        skinned,
    }
}

fn make_skinned(
    gpu: &WgpuPresenter,
    pbr: &PbrResources,
    item: &PbrDrawItem,
    slot: &klotho_manifest::PaletteSlot,
) -> Prepared {
    let model = model_from_pose(item.pose);
    let alb = albedo(item.mat);
    let (metalness, roughness) = metalness_roughness(item.mat.tag);
    let ub = pack_object(&model, alb, item.mat.tag as u32, metalness, roughness);
    let buf = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pbr-object-ub"),
            contents: &ub,
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let pal = pack_palette(&slot.joints);
    let palette_buf = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pbr-palette-ub"),
            contents: &pal,
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pbr.skinned_object_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: palette_buf.as_entire_binding(),
            },
        ],
    });
    Prepared {
        blob: item.blob,
        bg,
        _buf: buf,
        _palette_buf: Some(palette_buf),
        skinned: true,
    }
}

const PALETTE_BONES: usize = 256;
const PALETTE_BYTES: usize = PALETTE_BONES * 64;

fn pack_palette(joints: &[klotho_core::PoseMm]) -> [u8; PALETTE_BYTES] {
    let mut b = [0u8; PALETTE_BYTES];
    let id = crate::math::identity();
    for i in 0..PALETTE_BONES {
        put_mat4(&mut b, i * 64, &id);
    }
    for (i, pose) in joints.iter().take(PALETTE_BONES).enumerate() {
        put_mat4(&mut b, i * 64, &model_from_pose(*pose));
    }
    b
}

fn draw_prepared(
    pass: &mut wgpu::RenderPass<'_>,
    gpu: &WgpuPresenter,
    prepared: &[Prepared],
    instances: std::ops::Range<u32>,
    shadow: bool,
) {
    let pbr = gpu.pbr.as_ref().expect("pbr");
    for skinned in [false, true] {
        let pipeline = match (shadow, skinned) {
            (false, false) => &pbr.pipeline,
            (false, true) => &pbr.skinned_pipeline,
            (true, false) => &pbr.shadow_pipeline,
            (true, true) => &pbr.skinned_shadow_pipeline,
        };
        pass.set_pipeline(pipeline);
        for item in prepared.iter().filter(|p| p.skinned == skinned) {
            let mesh = if skinned {
                gpu.skinned_meshes.get(&item.blob)
            } else {
                gpu.meshes.get(&item.blob)
            };
            let Some(mesh) = mesh else {
                continue;
            };
            pass.set_bind_group(1, &item.bg, &[]);
            pass.set_vertex_buffer(0, mesh.verts.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.nidx, 0, instances.clone());
        }
    }
}

fn blit_bg(
    gpu: &WgpuPresenter,
    pbr: &PbrResources,
    src: &wgpu::TextureView,
    post_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pbr-blit-bg"),
        layout: &pbr.blit_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(src),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pbr.linear_samp),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: post_buf.as_entire_binding(),
            },
        ],
    })
}

fn ssgi_bg(gpu: &WgpuPresenter, pbr: &PbrResources) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pbr-ssgi-bg"),
        layout: &pbr.ssgi_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&pbr.pbr_color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pbr.linear_samp),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: pbr.post_half_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&pbr.pbr_depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&pbr.nearest_samp),
            },
        ],
    })
}

fn composite_bg(gpu: &WgpuPresenter, pbr: &PbrResources) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pbr-composite-bg"),
        layout: &pbr.composite_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&pbr.pbr_color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pbr.linear_samp),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: pbr.post_full_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&pbr.ssgi_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&pbr.bloom_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&pbr.history_view),
            },
        ],
    })
}

fn load_store(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    }
}

fn write_post(queue: &wgpu::Queue, buf: &wgpu::Buffer, w: u32, h: u32, flags: u32) {
    let mut b = [0u8; 16];
    put_f32(&mut b, 0, w as f32);
    put_f32(&mut b, 4, h as f32);
    put_u32(&mut b, 8, flags);
    queue.write_buffer(buf, 0, &b);
}

fn sun_dir_intensity(vis: &VisualManifest) -> ([f32; 3], f32) {
    for l in &vis.lights {
        if let LightKind::Sun {
            dir,
            intensity_milli,
        } = l.kind
        {
            let v = [dir.x as f32, dir.y as f32, dir.z as f32];
            let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-8);
            return (
                [v[0] / n, v[1] / n, v[2] / n],
                f32::from(intensity_milli) / 1000.0,
            );
        }
    }
    let v = [0.35f32, 0.8, 0.45];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-8);
    ([v[0] / n, v[1] / n, v[2] / n], 1.0)
}

#[allow(clippy::too_many_arguments)]
fn pack_frame(
    vp: &[f32; 16],
    cascades: &[[f32; 16]; 3],
    sun_dir: [f32; 3],
    sun_i: f32,
    eye: [f32; 3],
    splits: [f32; 4],
    tiles: &TileAssign,
    light_count: u32,
    width: u32,
    height: u32,
    n_cascades: u8,
    probe: Option<&klotho_manifest::ProbeGrid>,
) -> [u8; FRAME_BYTES as usize] {
    let mut b = [0u8; FRAME_BYTES as usize];
    put_mat4(&mut b, 0, vp);
    put_mat4(&mut b, 64, &cascades[0]);
    put_mat4(&mut b, 128, &cascades[1]);
    put_mat4(&mut b, 192, &cascades[2]);
    put_f32(&mut b, 256, sun_dir[0]);
    put_f32(&mut b, 260, sun_dir[1]);
    put_f32(&mut b, 264, sun_dir[2]);
    put_f32(&mut b, 268, sun_i);
    put_f32(&mut b, 272, eye[0]);
    put_f32(&mut b, 276, eye[1]);
    put_f32(&mut b, 280, eye[2]);
    put_f32(&mut b, 288, splits[0]);
    put_f32(&mut b, 292, splits[1]);
    put_f32(&mut b, 296, splits[2]);
    put_f32(&mut b, 300, splits[3]);
    put_u32(&mut b, 304, tiles.tiles_x);
    put_u32(&mut b, 308, tiles.tiles_y);
    put_u32(&mut b, 312, light_count.min(MAX_POINT_LIGHTS as u32));
    put_u32(&mut b, 316, u32::from(n_cascades));
    put_f32(&mut b, 320, width as f32);
    put_f32(&mut b, 324, height as f32);
    put_f32(&mut b, 328, width as f32 / tiles.tiles_x.max(1) as f32);
    put_f32(&mut b, 332, height as f32 / tiles.tiles_y.max(1) as f32);
    if let Some(g) = probe {
        put_f32(&mut b, 336, g.origin.x as f32);
        put_f32(&mut b, 340, g.origin.y as f32);
        put_f32(&mut b, 344, g.origin.z as f32);
        put_f32(&mut b, 348, g.spacing_mm as f32);
        put_u32(&mut b, 352, u32::from(g.dim.0));
        put_u32(&mut b, 356, u32::from(g.dim.1));
        put_u32(&mut b, 360, u32::from(g.dim.2));
        put_u32(&mut b, 364, 1);
    }
    b
}

fn pack_lights(lights: &[PointLight]) -> [u8; LIGHTS_BYTES as usize] {
    let mut b = [0u8; LIGHTS_BYTES as usize];
    for (i, l) in lights.iter().take(MAX_POINT_LIGHTS).enumerate() {
        let o = i * 32;
        put_f32(&mut b, o, l.pos[0]);
        put_f32(&mut b, o + 4, l.pos[1]);
        put_f32(&mut b, o + 8, l.pos[2]);
        put_f32(&mut b, o + 12, l.radius);
        put_f32(&mut b, o + 16, l.color[0]);
        put_f32(&mut b, o + 20, l.color[1]);
        put_f32(&mut b, o + 24, l.color[2]);
        put_f32(&mut b, o + 28, l.intensity);
    }
    b
}

fn pack_tiles(tiles: &TileAssign) -> [u8; TILES_BYTES as usize] {
    let mut b = [0u8; TILES_BYTES as usize];
    for (i, m) in tiles.masks.iter().enumerate() {
        let o = i * 4;
        if o + 4 <= b.len() {
            put_u32(&mut b, o, *m);
        }
    }
    b
}

fn pack_object(
    model: &[f32; 16],
    alb: [f32; 4],
    tag: u32,
    metalness: f32,
    roughness: f32,
) -> [u8; 96] {
    let mut b = [0u8; 96];
    put_mat4(&mut b, 0, model);
    put_f32(&mut b, 64, alb[0]);
    put_f32(&mut b, 68, alb[1]);
    put_f32(&mut b, 72, alb[2]);
    put_f32(&mut b, 76, alb[3]);
    put_u32(&mut b, 80, tag);
    put_f32(&mut b, 84, metalness);
    put_f32(&mut b, 88, roughness);
    b
}

fn put_f32(buf: &mut [u8], off: usize, v: f32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_mat4(buf: &mut [u8], off: usize, m: &[f32; 16]) {
    for (i, v) in m.iter().enumerate() {
        put_f32(buf, off + i * 4, *v);
    }
}

fn ub_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn tex_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    sample_type: wgpu::TextureSampleType,
    view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn post_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            tex_entry(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Float { filterable: true },
                wgpu::TextureViewDimension::D2,
            ),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            ub_entry(2, wgpu::ShaderStages::FRAGMENT),
        ],
    })
}

fn ssgi_bg_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("pbr-ssgi-layout"),
        entries: &[
            tex_entry(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Float { filterable: true },
                wgpu::TextureViewDimension::D2,
            ),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            ub_entry(2, wgpu::ShaderStages::FRAGMENT),
            tex_entry(
                3,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Depth,
                wgpu::TextureViewDimension::D2,
            ),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    })
}

fn composite_bg_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let tex = |b| {
        tex_entry(
            b,
            wgpu::ShaderStages::FRAGMENT,
            wgpu::TextureSampleType::Float { filterable: true },
            wgpu::TextureViewDimension::D2,
        )
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("pbr-composite-layout"),
        entries: &[
            tex(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            ub_entry(2, wgpu::ShaderStages::FRAGMENT),
            tex(5),
            tex(6),
            tex(7),
        ],
    })
}

const VTX_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
const SKIN_VTX_ATTRS: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 2,
    },
];

fn vert_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VTX_ATTRS,
    }
}

fn skinned_vert_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 48,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &SKIN_VTX_ATTRS,
    }
}

#[allow(clippy::too_many_arguments)]
fn color_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
    depth: bool,
    label: &str,
    verts: wgpu::VertexBufferLayout<'static>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[verts],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
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
        depth_stencil: depth.then_some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fs: &str,
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn ubuf(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn shadow_layer_view(tex: &wgpu::Texture, layer: u32) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor {
        label: Some("pbr-shadow-layer"),
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

fn color_target(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn depth_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pbr-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn make_probe(device: &wgpu::Device, queue: &wgpu::Queue) -> (wgpu::Texture, wgpu::TextureView) {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pbr-probe"),
        size: wgpu::Extent3d {
            width: PROBE_DIM,
            height: PROBE_DIM,
            depth_or_array_layers: PROBE_DIM,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let row = 256u32;
    let mut data = vec![0u8; (row * PROBE_DIM * PROBE_DIM) as usize];
    for z in 0..PROBE_DIM {
        for y in 0..PROBE_DIM {
            for x in 0..PROBE_DIM {
                let off = (z * row * PROBE_DIM + y * row + x * 4) as usize;
                data[off] = 90;
                data[off + 1] = 110;
                data[off + 2] = 140;
                data[off + 3] = 255;
            }
        }
    }
    queue.write_texture(
        t.as_image_copy(),
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row),
            rows_per_image: Some(PROBE_DIM),
        },
        wgpu::Extent3d {
            width: PROBE_DIM,
            height: PROBE_DIM,
            depth_or_array_layers: PROBE_DIM,
        },
    );
    let v = t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D3),
        ..Default::default()
    });
    (t, v)
}

#[cfg(test)]
mod tests {
    use klotho_core::{BlobId, Epoch, Mm, PoseMm, Tick, YawMd};
    use klotho_manifest::{
        GpuHandle, MaterialRef, MaterialTag, PaletteSlot, PostFlags, SkinnedInstance,
        VisualManifest,
    };

    use super::pbr_draw_items;

    fn mat() -> MaterialRef {
        MaterialRef {
            tag: MaterialTag::Stone,
            palette: 0,
        }
    }

    #[test]
    fn skinned_instances_are_drawn() {
        let opaque = BlobId::from_bytes([1; 32]);
        let skinned = BlobId::from_bytes([2; 32]);
        let vis = VisualManifest::from_v2(
            Epoch::ZERO,
            Tick::ZERO,
            [(opaque, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO), mat())],
            [],
            [SkinnedInstance {
                blob: skinned,
                gpu: GpuHandle::NONE,
                pose: PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd::ZERO),
                palette: 0,
                material: mat(),
            }],
            [PaletteSlot {
                gpu: GpuHandle::NONE,
                bones: 16,
                joints: vec![PoseMm::default(); 16],
            }],
            [],
            [],
            PostFlags::ADVENTURE,
            [],
        );
        assert_eq!(vis.skinned.len(), 1);
        let items = pbr_draw_items(&vis, &[0]);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].blob, opaque);
        assert_eq!(items[1].blob, skinned);
        assert_eq!(items[1].palette, Some(0));
    }

    #[test]
    fn palette_index_oob_is_not_drawn() {
        let skinned = BlobId::from_bytes([2; 32]);
        let vis = VisualManifest::from_v2(
            Epoch::ZERO,
            Tick::ZERO,
            [],
            [],
            [SkinnedInstance {
                blob: skinned,
                gpu: GpuHandle::NONE,
                pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
                palette: 3,
                material: mat(),
            }],
            [PaletteSlot::identity()],
            [],
            [],
            PostFlags::ADVENTURE,
            [],
        );
        let items = pbr_draw_items(&vis, &[]);
        assert!(items.is_empty());
    }

    #[test]
    fn bones_zero_identity_is_drawn() {
        let skinned = BlobId::from_bytes([9; 32]);
        let vis = VisualManifest::from_v2(
            Epoch::ZERO,
            Tick::ZERO,
            [],
            [],
            [SkinnedInstance {
                blob: skinned,
                gpu: GpuHandle::NONE,
                pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
                palette: 0,
                material: mat(),
            }],
            [PaletteSlot::identity()],
            [],
            [],
            PostFlags::COMPETITIVE,
            [],
        );
        let items = pbr_draw_items(&vis, &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].blob, skinned);
        assert_eq!(vis.palettes[0].bones, 0);
    }
}

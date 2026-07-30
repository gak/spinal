use std::{collections::HashMap, ops::Range};

use bevy::{
    app::{App, PostUpdate},
    asset::{AssetEvent, AssetId, Assets, Handle},
    camera::visibility::{ViewVisibility, VisibilitySystems},
    color::Color,
    core_pipeline::{
        core_2d::{CORE_2D_DEPTH_FORMAT, Transparent2d},
        tonemapping::{DebandDither, Tonemapping},
    },
    ecs::{
        message::MessageReader,
        query::ROQueryItem,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res, ResMut, SystemParamItem, lifetimeless::SRes},
    },
    gizmos::{
        GizmoPlugin,
        prelude::{AppGizmoBuilder, GizmoConfigStore, Gizmos},
    },
    image::{BevyDefault, Image},
    math::{FloatOrd, Vec2, Vec3},
    mesh::VertexBufferLayout,
    prelude::{GizmoConfigGroup as DeriveGizmoConfigGroup, GlobalTransform, Reflect, default},
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BlendState, BufferUsages, ColorTargetState, ColorWrites, CompareFunction,
            DepthBiasState, DepthStencilState, FragmentState, MultisampleState, PipelineCache,
            PrimitiveState, PrimitiveTopology, RawBufferVec, RenderPipelineDescriptor,
            SamplerBindingType, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StencilFaceState, StencilState, TextureFormat,
            TextureSampleType, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderDevice, RenderQueue},
        sync_component::SyncComponentPlugin,
        sync_world::{MainEntityHashMap, RenderEntity},
        texture::GpuImage,
        view::{ExtractedView, Msaa, RenderVisibleEntities, ViewTarget},
    },
    shader::{Shader, ShaderDefVal},
    sprite_render::{
        Mesh2dPipeline, Mesh2dPipelineKey, SetMesh2dViewBindGroup, init_mesh_2d_pipeline,
        tonemapping_pipeline_key,
    },
};

use crate::{
    SpinalAppearance, SpinalAsset, SpinalInstance, SpinalRuntimeConfig, plugin::SpinalSet,
    runtime::SpinalFrame,
};

const GPU_QUAD_FLOATS: usize = 24;
const GPU_QUAD_STRIDE: u64 = (GPU_QUAD_FLOATS * size_of::<f32>()) as u64;

type GpuQuad = [f32; GPU_QUAD_FLOATS];

const SPINAL_SHADER: &str = r"
#import bevy_sprite::mesh2d_view_bindings::view

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position_0: vec3<f32>,
    @location(1) position_1: vec3<f32>,
    @location(2) position_2: vec3<f32>,
    @location(3) position_3: vec3<f32>,
    @location(4) uv_0: vec2<f32>,
    @location(5) uv_1: vec2<f32>,
    @location(6) uv_2: vec2<f32>,
    @location(7) uv_3: vec2<f32>,
    @location(8) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vertex(in: VertexInput) -> VertexOutput {
    let positions = array<vec3<f32>, 4>(
        in.position_0,
        in.position_1,
        in.position_2,
        in.position_3,
    );
    let uvs = array<vec2<f32>, 4>(in.uv_0, in.uv_1, in.uv_2, in.uv_3);
    let corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = corners[in.vertex_index];

    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4<f32>(positions[corner], 1.0);
    out.uv = uvs[corner];
    out.color = in.color;
    return out;
}

@group(1) @binding(0) var spinal_texture: texture_2d<f32>;
@group(1) @binding(1) var spinal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(spinal_texture, spinal_sampler, in.uv) * in.color;

#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif

    return color;
}
";

#[derive(Default, Reflect, DeriveGizmoConfigGroup)]
struct SpinalIssueGizmos;

#[derive(Resource)]
struct SpinalShader(Handle<Shader>);

#[derive(Resource)]
struct SpinalPipeline {
    mesh2d_pipeline: Mesh2dPipeline,
    shader: Handle<Shader>,
    texture_layout: BindGroupLayoutDescriptor,
}

#[derive(Clone, Copy)]
struct ExtractedQuad {
    positions: [Vec3; 4],
    uvs: [Vec2; 4],
    color: [f32; 4],
    image: AssetId<Image>,
}

struct ExtractedFrame {
    render_entity: bevy::ecs::entity::Entity,
    sort_key: f32,
    quads: Range<usize>,
}

#[derive(Resource, Default)]
struct ExtractedSpinalFrames {
    frames: MainEntityHashMap<ExtractedFrame>,
    quads: Vec<ExtractedQuad>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AdjacentBatch<K> {
    key: K,
    instances: Range<u32>,
}

type PreparedBatch = AdjacentBatch<AssetId<Image>>;

struct PreparedFrame {
    batches: Range<usize>,
}

#[derive(Resource, Default)]
struct PreparedSpinalFrames {
    frames: MainEntityHashMap<PreparedFrame>,
    batches: Vec<PreparedBatch>,
}

#[derive(Resource)]
struct SpinalMeta {
    instances: RawBufferVec<GpuQuad>,
}

impl Default for SpinalMeta {
    fn default() -> Self {
        let mut instances = RawBufferVec::new(BufferUsages::VERTEX);
        instances.set_label(Some("spinal instance buffer"));
        Self { instances }
    }
}

#[derive(Resource, Default)]
struct SpinalImageBindGroups {
    values: HashMap<AssetId<Image>, BindGroup>,
}

#[derive(Resource, Default)]
struct SpinalImageEvents {
    images: Vec<AssetEvent<Image>>,
}

type DrawSpinal = (SetItemPipeline, SetMesh2dViewBindGroup<0>, DrawSpinalFrame);

pub(crate) fn install_render(app: &mut App) {
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }

    app.add_plugins(SyncComponentPlugin::<SpinalInstance>::default());

    if app.is_plugin_added::<GizmoPlugin>() {
        app.init_gizmo_group::<SpinalIssueGizmos>().add_systems(
            PostUpdate,
            (configure_issue_gizmos, draw_issue_crosses)
                .chain()
                .after(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible)
                .in_set(SpinalSet::Render),
        );
    }

    let shader = {
        let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
        shaders.add(Shader::from_wgsl(SPINAL_SHADER, file!()))
    };

    let render_app = app
        .get_sub_app_mut(RenderApp)
        .expect("the render app was checked before loading the shader");
    render_app
        .insert_resource(SpinalShader(shader))
        .init_resource::<ExtractedSpinalFrames>()
        .init_resource::<PreparedSpinalFrames>()
        .init_resource::<SpinalMeta>()
        .init_resource::<SpinalImageBindGroups>()
        .init_resource::<SpinalImageEvents>()
        .init_resource::<SpecializedRenderPipelines<SpinalPipeline>>()
        .add_render_command::<Transparent2d, DrawSpinal>()
        .add_systems(
            RenderStartup,
            init_spinal_pipeline.after(init_mesh_2d_pipeline),
        )
        .add_systems(
            ExtractSchedule,
            (extract_spinal_frames, extract_image_events),
        )
        .add_systems(
            Render,
            (
                queue_spinal_frames.in_set(RenderSystems::Queue),
                prepare_spinal_frames.in_set(RenderSystems::PrepareResources),
            ),
        );
}

fn init_spinal_pipeline(
    mut commands: Commands,
    mesh2d_pipeline: Res<Mesh2dPipeline>,
    shader: Res<SpinalShader>,
) {
    let texture_layout = BindGroupLayoutDescriptor::new(
        "spinal texture layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );

    commands.insert_resource(SpinalPipeline {
        mesh2d_pipeline: mesh2d_pipeline.clone(),
        shader: shader.0.clone(),
        texture_layout,
    });
}

impl SpecializedRenderPipeline for SpinalPipeline {
    type Key = Mesh2dPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let shader_defs = shader_defs(key);
        let format = if key.contains(Mesh2dPipelineKey::HDR) {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };

        RenderPipelineDescriptor {
            label: Some("spinal pipeline".into()),
            layout: vec![
                self.mesh2d_pipeline.view_layout.clone(),
                self.texture_layout.clone(),
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: shader_defs.clone(),
                buffers: vec![gpu_quad_layout()],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: spinal_primitive_state(),
            depth_stencil: Some(DepthStencilState {
                format: CORE_2D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            }),
            multisample: MultisampleState {
                count: key.msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            ..default()
        }
    }
}

fn shader_defs(key: Mesh2dPipelineKey) -> Vec<ShaderDefVal> {
    let mut shader_defs = Vec::new();
    if !key.contains(Mesh2dPipelineKey::TONEMAP_IN_SHADER) {
        return shader_defs;
    }

    shader_defs.push("TONEMAP_IN_SHADER".into());
    shader_defs.push(ShaderDefVal::UInt(
        "TONEMAPPING_LUT_TEXTURE_BINDING_INDEX".into(),
        2,
    ));
    shader_defs.push(ShaderDefVal::UInt(
        "TONEMAPPING_LUT_SAMPLER_BINDING_INDEX".into(),
        3,
    ));

    let method = key.intersection(Mesh2dPipelineKey::TONEMAP_METHOD_RESERVED_BITS);
    match method {
        Mesh2dPipelineKey::TONEMAP_METHOD_NONE => {
            shader_defs.push("TONEMAP_METHOD_NONE".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_REINHARD => {
            shader_defs.push("TONEMAP_METHOD_REINHARD".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_REINHARD_LUMINANCE => {
            shader_defs.push("TONEMAP_METHOD_REINHARD_LUMINANCE".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_ACES_FITTED => {
            shader_defs.push("TONEMAP_METHOD_ACES_FITTED".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_AGX => {
            shader_defs.push("TONEMAP_METHOD_AGX".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_SOMEWHAT_BORING_DISPLAY_TRANSFORM => {
            shader_defs.push("TONEMAP_METHOD_SOMEWHAT_BORING_DISPLAY_TRANSFORM".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_TONY_MC_MAPFACE => {
            shader_defs.push("TONEMAP_METHOD_TONY_MC_MAPFACE".into());
        }
        Mesh2dPipelineKey::TONEMAP_METHOD_BLENDER_FILMIC => {
            shader_defs.push("TONEMAP_METHOD_BLENDER_FILMIC".into());
        }
        _ => {}
    }

    if key.contains(Mesh2dPipelineKey::DEBAND_DITHER) {
        shader_defs.push("DEBAND_DITHER".into());
    }
    shader_defs
}

fn gpu_quad_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: GPU_QUAD_STRIDE,
        step_mode: VertexStepMode::Instance,
        attributes: vec![
            vertex_attribute(VertexFormat::Float32x3, 0, 0),
            vertex_attribute(VertexFormat::Float32x3, 12, 1),
            vertex_attribute(VertexFormat::Float32x3, 24, 2),
            vertex_attribute(VertexFormat::Float32x3, 36, 3),
            vertex_attribute(VertexFormat::Float32x2, 48, 4),
            vertex_attribute(VertexFormat::Float32x2, 56, 5),
            vertex_attribute(VertexFormat::Float32x2, 64, 6),
            vertex_attribute(VertexFormat::Float32x2, 72, 7),
            vertex_attribute(VertexFormat::Float32x4, 80, 8),
        ],
    }
}

const fn vertex_attribute(
    format: VertexFormat,
    offset: u64,
    shader_location: u32,
) -> VertexAttribute {
    VertexAttribute {
        format,
        offset,
        shader_location,
    }
}

fn spinal_primitive_state() -> PrimitiveState {
    PrimitiveState {
        topology: PrimitiveTopology::TriangleList,
        cull_mode: None,
        ..default()
    }
}

#[allow(clippy::type_complexity)]
fn extract_spinal_frames(
    mut extracted: ResMut<ExtractedSpinalFrames>,
    assets: Extract<Res<Assets<SpinalAsset>>>,
    query: Extract<
        Query<(
            bevy::ecs::entity::Entity,
            RenderEntity,
            &ViewVisibility,
            &GlobalTransform,
            &SpinalInstance,
            &SpinalAppearance,
            &SpinalFrame,
        )>,
    >,
) {
    extracted.frames.clear();
    extracted.quads.clear();

    for (entity, render_entity, visibility, transform, instance, appearance, frame) in &query {
        if !visibility.get() || !frame.ready {
            continue;
        }
        let Some(asset) = assets.get(instance.asset()) else {
            continue;
        };
        let sort_key = transform.translation().z;
        if !sort_key.is_finite() {
            continue;
        }

        let start = extracted.quads.len();
        let mut complete = true;
        for draw in &frame.draws {
            let Some(page) = asset.page(draw.page_ordinal) else {
                complete = false;
                break;
            };
            let Some(positions) = transform_positions(
                transform,
                draw.positions,
                appearance.flip_x(),
                appearance.flip_y(),
            ) else {
                complete = false;
                break;
            };
            let Some(color) = modulated_linear_color(draw.color, appearance.modulation()) else {
                complete = false;
                break;
            };
            if !draw.uvs.iter().all(|uv| uv.is_finite()) {
                complete = false;
                break;
            }
            extracted.quads.push(ExtractedQuad {
                positions,
                uvs: draw.uvs,
                color,
                image: page.image().id(),
            });
        }
        if !complete {
            extracted.quads.truncate(start);
            continue;
        }
        let end = extracted.quads.len();
        if start == end {
            continue;
        }

        extracted.frames.insert(
            entity.into(),
            ExtractedFrame {
                render_entity,
                sort_key,
                quads: start..end,
            },
        );
    }
}

fn transform_positions(
    transform: &GlobalTransform,
    positions: [Vec2; 4],
    flip_x: bool,
    flip_y: bool,
) -> Option<[Vec3; 4]> {
    let facing = local_facing(flip_x, flip_y);
    let mut transformed = [Vec3::ZERO; 4];
    for (target, source) in transformed.iter_mut().zip(positions) {
        let position = transform.transform_point((source * facing).extend(0.0));
        if !position.is_finite() {
            return None;
        }
        *target = position;
    }
    Some(transformed)
}

const fn local_facing(flip_x: bool, flip_y: bool) -> Vec2 {
    Vec2::new(
        if flip_x { -1.0 } else { 1.0 },
        if flip_y { -1.0 } else { 1.0 },
    )
}

fn modulated_linear_color(authored: [f32; 4], modulation: Color) -> Option<[f32; 4]> {
    if !authored.iter().all(|channel| channel.is_finite()) {
        return None;
    }
    let authored = Color::srgba(authored[0], authored[1], authored[2], authored[3]).to_linear();
    let modulation = modulation.to_linear();
    let composed = [
        authored.red * modulation.red,
        authored.green * modulation.green,
        authored.blue * modulation.blue,
        authored.alpha * modulation.alpha,
    ];
    composed
        .iter()
        .all(|channel| channel.is_finite())
        .then_some(composed)
}

fn extract_image_events(
    mut extracted: ResMut<SpinalImageEvents>,
    mut image_events: Extract<MessageReader<AssetEvent<Image>>>,
) {
    extracted.images.clear();
    extracted.images.extend(image_events.read().copied());
}

#[allow(clippy::type_complexity)]
fn queue_spinal_frames(
    draw_functions: Res<DrawFunctions<Transparent2d>>,
    pipeline: Res<SpinalPipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<SpinalPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    extracted: Res<ExtractedSpinalFrames>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    views: Query<(
        &RenderVisibleEntities,
        &ExtractedView,
        &Msaa,
        Option<&Tonemapping>,
        Option<&DebandDither>,
    )>,
) {
    if extracted.frames.is_empty() {
        return;
    }

    let draw_function = draw_functions.read().id::<DrawSpinal>();
    for (visible_entities, view, msaa, tonemapping, dither) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let pipeline_key = pipeline_key(view, msaa, tonemapping, dither);
        let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, pipeline_key);

        phase.items.reserve(extracted.frames.len());
        for (render_entity, main_entity) in visible_entities.iter::<SpinalInstance>() {
            let Some(frame) = extracted.frames.get(main_entity) else {
                continue;
            };
            if frame.render_entity != *render_entity {
                continue;
            }
            phase.add(Transparent2d {
                entity: (*render_entity, *main_entity),
                draw_function,
                pipeline: pipeline_id,
                sort_key: FloatOrd(frame.sort_key),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                extracted_index: usize::MAX,
                indexed: false,
            });
        }
    }
}

fn pipeline_key(
    view: &ExtractedView,
    msaa: &Msaa,
    tonemapping: Option<&Tonemapping>,
    dither: Option<&DebandDither>,
) -> Mesh2dPipelineKey {
    let mut key = Mesh2dPipelineKey::from_msaa_samples(msaa.samples())
        | Mesh2dPipelineKey::from_hdr(view.hdr)
        | Mesh2dPipelineKey::BLEND_ALPHA
        | Mesh2dPipelineKey::from_primitive_topology(PrimitiveTopology::TriangleList);

    if !view.hdr {
        if let Some(tonemapping) = tonemapping {
            key |= Mesh2dPipelineKey::TONEMAP_IN_SHADER;
            key |= tonemapping_pipeline_key(*tonemapping);
        }
        if matches!(dither, Some(DebandDither::Enabled)) {
            key |= Mesh2dPipelineKey::DEBAND_DITHER;
        }
    }
    key
}

#[allow(clippy::too_many_arguments)]
fn prepare_spinal_frames(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<SpinalPipeline>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    image_events: Res<SpinalImageEvents>,
    extracted: Res<ExtractedSpinalFrames>,
    mut prepared: ResMut<PreparedSpinalFrames>,
    mut image_bind_groups: ResMut<SpinalImageBindGroups>,
    mut meta: ResMut<SpinalMeta>,
) {
    for event in &image_events.images {
        match event {
            AssetEvent::Added { .. } | AssetEvent::LoadedWithDependencies { .. } => {}
            AssetEvent::Unused { id }
            | AssetEvent::Modified { id }
            | AssetEvent::Removed { id } => {
                image_bind_groups.values.remove(id);
            }
        }
    }

    prepared.frames.clear();
    prepared.batches.clear();
    prepared.batches.reserve(extracted.quads.len());
    meta.instances.clear();
    meta.instances.reserve_internal(extracted.quads.len());

    for (main_entity, frame) in &extracted.frames {
        let quads = &extracted.quads[frame.quads.clone()];
        if quads.is_empty()
            || quads
                .iter()
                .any(|quad| gpu_images.get(quad.image).is_none())
        {
            continue;
        }

        let batch_start = prepared.batches.len();
        for quad in quads {
            let gpu_image = gpu_images
                .get(quad.image)
                .expect("the complete frame was checked before preparation");
            image_bind_groups
                .values
                .entry(quad.image)
                .or_insert_with(|| {
                    render_device.create_bind_group(
                        "spinal texture bind group",
                        &pipeline_cache.get_bind_group_layout(&pipeline.texture_layout),
                        &BindGroupEntries::sequential((
                            &gpu_image.texture_view,
                            &gpu_image.sampler,
                        )),
                    )
                });

            let instance = u32::try_from(meta.instances.push(pack_quad(quad)))
                .expect("a render frame cannot contain more than u32::MAX quads");
            push_adjacent_batch(&mut prepared.batches, batch_start, quad.image, instance);
        }
        let batch_end = prepared.batches.len();
        prepared.frames.insert(
            *main_entity,
            PreparedFrame {
                batches: batch_start..batch_end,
            },
        );
    }

    meta.instances.write_buffer(&render_device, &render_queue);
}

fn pack_quad(quad: &ExtractedQuad) -> GpuQuad {
    let mut packed = [0.0; GPU_QUAD_FLOATS];
    for (index, position) in quad.positions.iter().enumerate() {
        let offset = index * 3;
        packed[offset..offset + 3].copy_from_slice(&position.to_array());
    }
    for (index, uv) in quad.uvs.iter().enumerate() {
        let offset = 12 + index * 2;
        packed[offset..offset + 2].copy_from_slice(&uv.to_array());
    }
    packed[20..24].copy_from_slice(&quad.color);
    packed
}

fn push_adjacent_batch<K: Copy + Eq>(
    batches: &mut Vec<AdjacentBatch<K>>,
    frame_start: usize,
    key: K,
    instance: u32,
) {
    if let Some(batch) = batches
        .get_mut(frame_start..)
        .and_then(|frame_batches| frame_batches.last_mut())
        .filter(|batch| batch.key == key && batch.instances.end == instance)
    {
        batch.instances.end += 1;
        return;
    }

    batches.push(AdjacentBatch {
        key,
        instances: instance..instance + 1,
    });
}

struct DrawSpinalFrame;

impl<P: PhaseItem> RenderCommand<P> for DrawSpinalFrame {
    type Param = (
        SRes<SpinalMeta>,
        SRes<PreparedSpinalFrames>,
        SRes<SpinalImageBindGroups>,
    );
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (meta, prepared, image_bind_groups): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let meta = meta.into_inner();
        let prepared = prepared.into_inner();
        let image_bind_groups = image_bind_groups.into_inner();
        let Some(frame) = prepared.frames.get(&item.main_entity()) else {
            return RenderCommandResult::Skip;
        };
        let batches = &prepared.batches[frame.batches.clone()];
        let Some(buffer) = meta.instances.buffer() else {
            return RenderCommandResult::Skip;
        };
        if batches
            .iter()
            .any(|batch| !image_bind_groups.values.contains_key(&batch.key))
        {
            return RenderCommandResult::Skip;
        }

        pass.set_vertex_buffer(0, buffer.slice(..));
        for batch in batches {
            let bind_group = &image_bind_groups.values[&batch.key];
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw(0..6, batch.instances.clone());
        }
        RenderCommandResult::Success
    }
}

fn draw_issue_crosses(
    mut gizmos: Gizmos<SpinalIssueGizmos>,
    config: Res<SpinalRuntimeConfig>,
    query: Query<(
        &ViewVisibility,
        &GlobalTransform,
        &SpinalAppearance,
        &SpinalFrame,
    )>,
) {
    let half_extent = config.diagnostic_marker_size() * 0.5;
    for (visibility, transform, appearance, frame) in &query {
        if !visibility.get() {
            continue;
        }
        let facing = local_facing(appearance.flip_x(), appearance.flip_y());
        for point in &frame.issue_points {
            for (start, end) in issue_cross_segments(*point, half_extent) {
                let start = transform.transform_point((start * facing).extend(0.0));
                let end = transform.transform_point((end * facing).extend(0.0));
                if start.is_finite() && end.is_finite() {
                    gizmos.line(start, end, bevy::color::palettes::css::RED);
                }
            }
        }
    }
}

fn configure_issue_gizmos(
    runtime_config: Res<SpinalRuntimeConfig>,
    mut gizmo_configs: ResMut<GizmoConfigStore>,
) {
    let (gizmo_config, _group) = gizmo_configs.config_mut::<SpinalIssueGizmos>();
    gizmo_config.enabled = runtime_config.diagnostic_markers();
    gizmo_config.line.width = runtime_config.diagnostic_marker_thickness();
}

fn issue_cross_segments(point: Vec2, half_extent: f32) -> [(Vec2, Vec2); 2] {
    [
        (
            point + Vec2::new(-half_extent, -half_extent),
            point + Vec2::new(half_extent, half_extent),
        ),
        (
            point + Vec2::new(-half_extent, half_extent),
            point + Vec2::new(half_extent, -half_extent),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_batches_preserve_non_adjacent_page_order() {
        let mut batches = Vec::new();
        for (instance, key) in ['A', 'A', 'B', 'A'].into_iter().enumerate() {
            push_adjacent_batch(&mut batches, 0, key, instance as u32);
        }

        assert_eq!(
            batches,
            vec![
                AdjacentBatch {
                    key: 'A',
                    instances: 0..2,
                },
                AdjacentBatch {
                    key: 'B',
                    instances: 2..3,
                },
                AdjacentBatch {
                    key: 'A',
                    instances: 3..4,
                },
            ]
        );
    }

    #[test]
    fn batches_do_not_merge_across_skeletons() {
        let mut batches = Vec::new();
        push_adjacent_batch(&mut batches, 0, 'A', 0);
        let second_frame_start = batches.len();
        push_adjacent_batch(&mut batches, second_frame_start, 'A', 1);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].instances, 0..1);
        assert_eq!(batches[1].instances, 1..2);
    }

    #[test]
    fn packed_quad_matches_the_declared_vertex_layout() {
        let quad = ExtractedQuad {
            positions: [
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::new(4.0, 5.0, 6.0),
                Vec3::new(7.0, 8.0, 9.0),
                Vec3::new(10.0, 11.0, 12.0),
            ],
            uvs: [
                Vec2::new(0.1, 0.2),
                Vec2::new(0.3, 0.4),
                Vec2::new(0.5, 0.6),
                Vec2::new(0.7, 0.8),
            ],
            color: [0.9, 0.8, 0.7, 0.6],
            image: AssetId::invalid(),
        };

        assert_eq!(
            pack_quad(&quad),
            [
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 0.1, 0.2, 0.3, 0.4,
                0.5, 0.6, 0.7, 0.8, 0.9, 0.8, 0.7, 0.6,
            ]
        );
        assert_eq!(gpu_quad_layout().array_stride, GPU_QUAD_STRIDE);
    }

    #[test]
    fn reflected_quads_are_not_culled() {
        assert_eq!(spinal_primitive_state().cull_mode, None);
    }

    #[test]
    fn local_facing_is_applied_before_the_entity_transform() {
        let transform = GlobalTransform::from_translation(Vec3::new(10.0, 20.0, 0.0));
        let positions = [
            Vec2::new(1.0, 2.0),
            Vec2::new(3.0, 4.0),
            Vec2::new(5.0, 6.0),
            Vec2::new(7.0, 8.0),
        ];

        assert_eq!(
            transform_positions(&transform, positions, true, false),
            Some([
                Vec3::new(9.0, 22.0, 0.0),
                Vec3::new(7.0, 24.0, 0.0),
                Vec3::new(5.0, 26.0, 0.0),
                Vec3::new(3.0, 28.0, 0.0),
            ])
        );
    }

    #[test]
    fn authored_and_instance_tints_are_composed_in_linear_space() {
        let color =
            modulated_linear_color([0.5, 0.25, 1.0, 0.8], Color::srgba(0.5, 1.0, 0.25, 0.5))
                .expect("finite colors can be rendered");
        let authored = Color::srgba(0.5, 0.25, 1.0, 0.8).to_linear();
        let instance = Color::srgba(0.5, 1.0, 0.25, 0.5).to_linear();

        assert_eq!(
            color,
            [
                authored.red * instance.red,
                authored.green * instance.green,
                authored.blue * instance.blue,
                authored.alpha * instance.alpha,
            ]
        );
        assert_ne!(color[0], 0.25, "raw sRGB channels must not reach the GPU");
    }

    #[test]
    fn issue_cross_is_centered_on_the_reported_point() {
        let point = Vec2::new(10.0, -5.0);
        let segments = issue_cross_segments(point, 2.0);

        for (start, end) in segments {
            assert_eq!((start + end) * 0.5, point);
        }
    }

    #[test]
    fn diagnostic_local_facing_matches_skeleton_facing() {
        let point = Vec2::new(10.0, -5.0);
        let facing = local_facing(true, false);
        let segments = issue_cross_segments(point, 2.0);

        for (start, end) in segments {
            assert_eq!((start * facing + end * facing) * 0.5, point * facing);
        }
    }
}

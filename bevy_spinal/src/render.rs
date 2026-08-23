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
    image::Image,
    math::{FloatOrd, Vec2, Vec3},
    mesh::VertexBufferLayout,
    prelude::{GizmoConfigGroup as DeriveGizmoConfigGroup, GlobalTransform, Reflect, default},
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BlendState, BufferUsages, ColorTargetState, ColorWrites, CompareFunction,
            DepthBiasState, DepthStencilState, FragmentState, IndexFormat, MultisampleState,
            PipelineCache, PrimitiveState, PrimitiveTopology, RawBufferVec,
            RenderPipelineDescriptor, SamplerBindingType, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StencilFaceState, StencilState, TextureSampleType,
            VertexAttribute, VertexFormat, VertexState, VertexStepMode,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderDevice, RenderQueue},
        sync_component::{SyncComponent, SyncComponentPlugin},
        sync_world::{MainEntityHashMap, RenderEntity},
        texture::GpuImage,
        view::{ExtractedView, Msaa, RenderVisibleEntities},
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

const GPU_VERTEX_FLOATS: usize = 9;
const GPU_VERTEX_STRIDE: u64 = (GPU_VERTEX_FLOATS * size_of::<f32>()) as u64;

type GpuVertex = [f32; GPU_VERTEX_FLOATS];

const SPINAL_SHADER: &str = r"
#import bevy_sprite::mesh2d_view_bindings::view

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif
#ifdef SRGB_OUTPUT
#import bevy_render::color_operations::linear_to_srgb
#endif
#ifdef OKLAB_OUTPUT
#import bevy_render::color_operations::linear_rgb_to_oklab
#endif

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
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
#ifdef SRGB_OUTPUT
    color = vec4(linear_to_srgb(color.rgb), color.a);
#endif
#ifdef OKLAB_OUTPUT
    color = vec4(linear_rgb_to_oklab(color.rgb), color.a);
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
struct ExtractedVertex {
    position: Vec3,
    uv: Vec2,
    color: [f32; 4],
}

#[derive(Clone)]
struct ExtractedDraw {
    image: AssetId<Image>,
    indices: Range<usize>,
}

struct ExtractedFrame {
    render_entity: bevy::ecs::entity::Entity,
    sort_key: f32,
    draws: Range<usize>,
}

#[derive(Resource, Default)]
struct ExtractedSpinalFrames {
    frames: MainEntityHashMap<ExtractedFrame>,
    vertices: Vec<ExtractedVertex>,
    indices: Vec<u32>,
    draws: Vec<ExtractedDraw>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AdjacentBatch<K> {
    key: K,
    indices: Range<u32>,
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
    vertices: RawBufferVec<GpuVertex>,
    indices: RawBufferVec<u32>,
}

impl Default for SpinalMeta {
    fn default() -> Self {
        let mut vertices = RawBufferVec::new(BufferUsages::VERTEX);
        vertices.set_label(Some("spinal vertex buffer"));
        let mut indices = RawBufferVec::new(BufferUsages::INDEX);
        indices.set_label(Some("spinal index buffer"));
        Self { vertices, indices }
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

impl SyncComponent for SpinalInstance {
    type Target = ();
}

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
        let format = key.target_format();

        RenderPipelineDescriptor {
            label: Some("spinal pipeline".into()),
            layout: vec![
                self.mesh2d_pipeline.view_layout.clone(),
                self.texture_layout.clone(),
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: shader_defs.clone(),
                buffers: vec![gpu_vertex_layout()],
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
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
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
        if key.contains(Mesh2dPipelineKey::SRGB_COMPOSITING) {
            shader_defs.push("SRGB_OUTPUT".into());
        }
        if key.contains(Mesh2dPipelineKey::OKLAB_COMPOSITING) {
            shader_defs.push("OKLAB_OUTPUT".into());
        }
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
        Mesh2dPipelineKey::TONEMAP_METHOD_PBR_NEUTRAL => {
            shader_defs.push("TONEMAP_METHOD_PBR_NEUTRAL".into());
        }
        _ => {}
    }

    if key.contains(Mesh2dPipelineKey::DEBAND_DITHER) {
        shader_defs.push("DEBAND_DITHER".into());
    }
    if key.contains(Mesh2dPipelineKey::SRGB_COMPOSITING) {
        shader_defs.push("SRGB_OUTPUT".into());
    }
    if key.contains(Mesh2dPipelineKey::OKLAB_COMPOSITING) {
        shader_defs.push("OKLAB_OUTPUT".into());
    }
    shader_defs
}

fn gpu_vertex_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: GPU_VERTEX_STRIDE,
        step_mode: VertexStepMode::Vertex,
        attributes: vec![
            vertex_attribute(VertexFormat::Float32x3, 0, 0),
            vertex_attribute(VertexFormat::Float32x2, 12, 1),
            vertex_attribute(VertexFormat::Float32x4, 20, 2),
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
    extracted.vertices.clear();
    extracted.indices.clear();
    extracted.draws.clear();

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

        let Some(draws) = append_extracted_frame_geometry(
            &mut extracted,
            transform,
            appearance,
            frame,
            |ordinal| asset.page(ordinal).map(|page| page.image().id()),
        ) else {
            continue;
        };

        extracted.frames.insert(
            entity.into(),
            ExtractedFrame {
                render_entity,
                sort_key,
                draws,
            },
        );
    }
}

fn append_extracted_frame_geometry(
    extracted: &mut ExtractedSpinalFrames,
    transform: &GlobalTransform,
    appearance: &SpinalAppearance,
    frame: &SpinalFrame,
    mut resolve_image: impl FnMut(usize) -> Option<AssetId<Image>>,
) -> Option<Range<usize>> {
    let vertex_start = extracted.vertices.len();
    let index_start = extracted.indices.len();
    let draw_start = extracted.draws.len();
    let result = (|| -> Option<Range<usize>> {
        for draw in &frame.draws {
            let image = resolve_image(draw.page_ordinal)?;
            let color = modulated_linear_color(draw.color, appearance.modulation())?;
            let source_vertices = frame.vertices.get(draw.vertices.clone())?;
            let source_indices = frame.indices.get(draw.indices.clone())?;
            if source_vertices.is_empty() || source_indices.is_empty() {
                return None;
            }

            let draw_vertex_start = extracted.vertices.len();
            for vertex in source_vertices {
                let position = transform_position(
                    transform,
                    vertex.position,
                    appearance.flip_x(),
                    appearance.flip_y(),
                )?;
                if !vertex.uv.is_finite() {
                    return None;
                }
                extracted.vertices.push(ExtractedVertex {
                    position,
                    uv: vertex.uv,
                    color,
                });
            }

            let draw_vertex_base = u32::try_from(draw_vertex_start).ok()?;
            let source_vertex_start = u32::try_from(draw.vertices.start).ok()?;
            let draw_index_start = extracted.indices.len();
            for source_index in source_indices {
                let local = source_index.checked_sub(source_vertex_start)?;
                if local as usize >= source_vertices.len() {
                    return None;
                }
                extracted.indices.push(draw_vertex_base.checked_add(local)?);
            }
            let draw_index_end = extracted.indices.len();
            extracted.draws.push(ExtractedDraw {
                image,
                indices: draw_index_start..draw_index_end,
            });
        }

        let draw_end = extracted.draws.len();
        (draw_start != draw_end).then_some(draw_start..draw_end)
    })();

    if result.is_none() {
        extracted.vertices.truncate(vertex_start);
        extracted.indices.truncate(index_start);
        extracted.draws.truncate(draw_start);
    }
    result
}

fn transform_position(
    transform: &GlobalTransform,
    position: Vec2,
    flip_x: bool,
    flip_y: bool,
) -> Option<Vec3> {
    let position = transform.transform_point((position * local_facing(flip_x, flip_y)).extend(0.0));
    position.is_finite().then_some(position)
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
        &ExtractedCamera,
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
    for (visible_entities, camera, view, msaa, tonemapping, dither) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let pipeline_key = pipeline_key(camera, view, msaa, tonemapping, dither);
        let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, pipeline_key);

        phase.items.reserve(extracted.frames.len());
        let Some(visible_entities) = visible_entities.get::<SpinalInstance>() else {
            continue;
        };
        for (render_entity, main_entity) in visible_entities.iter_visible() {
            let Some(frame) = extracted.frames.get(main_entity) else {
                continue;
            };
            if frame.render_entity != *render_entity {
                continue;
            }
            phase.add_transient(Transparent2d {
                entity: (*render_entity, *main_entity),
                draw_function,
                pipeline: pipeline_id,
                sort_key: FloatOrd(frame.sort_key),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                extracted_index: usize::MAX,
                indexed: true,
            });
        }
    }
}

fn pipeline_key(
    camera: &ExtractedCamera,
    view: &ExtractedView,
    msaa: &Msaa,
    tonemapping: Option<&Tonemapping>,
    dither: Option<&DebandDither>,
) -> Mesh2dPipelineKey {
    let mut key = Mesh2dPipelineKey::from_msaa_samples(msaa.samples())
        | Mesh2dPipelineKey::from_target_format(view.target_format)
        | Mesh2dPipelineKey::BLEND_ALPHA
        | Mesh2dPipelineKey::from_primitive_topology_and_strip_index(
            PrimitiveTopology::TriangleList,
            None,
        );

    if camera
        .compositing_space
        .is_some_and(|space| space == bevy::camera::CompositingSpace::Srgb)
    {
        key |= Mesh2dPipelineKey::SRGB_COMPOSITING;
    }
    if camera
        .compositing_space
        .is_some_and(|space| space == bevy::camera::CompositingSpace::Oklab)
    {
        key |= Mesh2dPipelineKey::OKLAB_COMPOSITING;
    }

    if !camera.hdr {
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
    prepared.batches.reserve(extracted.draws.len());
    meta.vertices.clear();
    meta.vertices.reserve_internal(extracted.vertices.len());
    for vertex in &extracted.vertices {
        meta.vertices.push(pack_vertex(vertex));
    }
    meta.indices.clear();
    meta.indices.reserve_internal(extracted.indices.len());
    for index in &extracted.indices {
        meta.indices.push(*index);
    }

    for (main_entity, frame) in &extracted.frames {
        let draws = &extracted.draws[frame.draws.clone()];
        if draws.is_empty()
            || draws
                .iter()
                .any(|draw| gpu_images.get(draw.image).is_none())
        {
            continue;
        }

        let batch_start = prepared.batches.len();
        for draw in draws {
            let gpu_image = gpu_images
                .get(draw.image)
                .expect("the complete frame was checked before preparation");
            image_bind_groups
                .values
                .entry(draw.image)
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

            let indices = u32::try_from(draw.indices.start)
                .expect("a render frame cannot contain more than u32::MAX indices")
                ..u32::try_from(draw.indices.end)
                    .expect("a render frame cannot contain more than u32::MAX indices");
            push_adjacent_batch(&mut prepared.batches, batch_start, draw.image, indices);
        }
        let batch_end = prepared.batches.len();
        prepared.frames.insert(
            *main_entity,
            PreparedFrame {
                batches: batch_start..batch_end,
            },
        );
    }

    meta.vertices.write_buffer(&render_device, &render_queue);
    meta.indices.write_buffer(&render_device, &render_queue);
}

fn pack_vertex(vertex: &ExtractedVertex) -> GpuVertex {
    let mut packed = [0.0; GPU_VERTEX_FLOATS];
    packed[0..3].copy_from_slice(&vertex.position.to_array());
    packed[3..5].copy_from_slice(&vertex.uv.to_array());
    packed[5..9].copy_from_slice(&vertex.color);
    packed
}

fn push_adjacent_batch<K: Copy + Eq>(
    batches: &mut Vec<AdjacentBatch<K>>,
    frame_start: usize,
    key: K,
    indices: Range<u32>,
) {
    if let Some(batch) = batches
        .get_mut(frame_start..)
        .and_then(|frame_batches| frame_batches.last_mut())
        .filter(|batch| batch.key == key && batch.indices.end == indices.start)
    {
        batch.indices.end = indices.end;
        return;
    }

    batches.push(AdjacentBatch { key, indices });
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
        let Some(vertex_buffer) = meta.vertices.buffer() else {
            return RenderCommandResult::Skip;
        };
        let Some(index_buffer) = meta.indices.buffer() else {
            return RenderCommandResult::Skip;
        };
        if batches
            .iter()
            .any(|batch| !image_bind_groups.values.contains_key(&batch.key))
        {
            return RenderCommandResult::Skip;
        }

        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), IndexFormat::Uint32);
        for batch in batches {
            let bind_group = &image_bind_groups.values[&batch.key];
            pass.set_bind_group(1, bind_group, &[]);
            pass.draw_indexed(batch.indices.clone(), 0, 0..1);
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
    fn manual_extraction_has_no_persistent_render_component_to_clean_up() {
        fn assert_empty_sync_target<T: SyncComponent<Target = ()>>() {}

        assert_empty_sync_target::<SpinalInstance>();
    }

    #[test]
    fn target_format_round_trips_through_the_mesh_pipeline_key() {
        for format in [
            bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb,
            bevy::render::render_resource::TextureFormat::Rgba16Float,
        ] {
            let key = Mesh2dPipelineKey::from_target_format(format);
            assert_eq!(key.target_format(), format);
        }
    }

    #[test]
    fn shader_definitions_cover_bevy_019_compositing_and_pbr_tonemapping() {
        assert!(shader_defs(Mesh2dPipelineKey::empty()).is_empty());
        assert_eq!(
            shader_defs(Mesh2dPipelineKey::SRGB_COMPOSITING),
            [ShaderDefVal::from("SRGB_OUTPUT")]
        );
        assert_eq!(
            shader_defs(Mesh2dPipelineKey::OKLAB_COMPOSITING),
            [ShaderDefVal::from("OKLAB_OUTPUT")]
        );

        let definitions = shader_defs(
            Mesh2dPipelineKey::TONEMAP_IN_SHADER
                | Mesh2dPipelineKey::TONEMAP_METHOD_PBR_NEUTRAL
                | Mesh2dPipelineKey::DEBAND_DITHER
                | Mesh2dPipelineKey::SRGB_COMPOSITING,
        );
        for definition in [
            ShaderDefVal::from("TONEMAP_IN_SHADER"),
            ShaderDefVal::from("TONEMAP_METHOD_PBR_NEUTRAL"),
            ShaderDefVal::from("DEBAND_DITHER"),
            ShaderDefVal::from("SRGB_OUTPUT"),
        ] {
            assert!(definitions.contains(&definition));
        }
    }

    #[test]
    fn adjacent_batches_preserve_non_adjacent_page_order() {
        let mut batches = Vec::new();
        for (draw, key) in ['A', 'A', 'B', 'A'].into_iter().enumerate() {
            let start = draw as u32 * 6;
            push_adjacent_batch(&mut batches, 0, key, start..start + 6);
        }

        assert_eq!(
            batches,
            vec![
                AdjacentBatch {
                    key: 'A',
                    indices: 0..12,
                },
                AdjacentBatch {
                    key: 'B',
                    indices: 12..18,
                },
                AdjacentBatch {
                    key: 'A',
                    indices: 18..24,
                },
            ]
        );
    }

    #[test]
    fn batches_do_not_merge_across_skeletons() {
        let mut batches = Vec::new();
        push_adjacent_batch(&mut batches, 0, 'A', 0..6);
        let second_frame_start = batches.len();
        push_adjacent_batch(&mut batches, second_frame_start, 'A', 6..12);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].indices, 0..6);
        assert_eq!(batches[1].indices, 6..12);
    }

    #[test]
    fn indexed_extraction_rebases_arbitrary_draws_and_preserves_page_order() {
        let mut images = Assets::<Image>::default();
        let page_a = images.add(Image::default()).id();
        let page_b = images.add(Image::default()).id();
        let mut extracted = ExtractedSpinalFrames::default();
        let prefix = SpinalFrame {
            revision: 1,
            draws: vec![crate::runtime::SpinalDraw {
                page_ordinal: 1,
                vertices: 0..3,
                indices: 0..3,
                color: [1.0; 4],
            }],
            vertices: vec![
                crate::runtime::SpinalVertex {
                    position: Vec2::ZERO,
                    uv: Vec2::ZERO,
                },
                crate::runtime::SpinalVertex {
                    position: Vec2::X,
                    uv: Vec2::X,
                },
                crate::runtime::SpinalVertex {
                    position: Vec2::Y,
                    uv: Vec2::Y,
                },
            ],
            indices: vec![0, 1, 2],
            issue_points: Vec::new(),
            ready: true,
        };
        let prefix_draws = append_extracted_frame_geometry(
            &mut extracted,
            &GlobalTransform::IDENTITY,
            &SpinalAppearance::default(),
            &prefix,
            |ordinal| match ordinal {
                0 => Some(page_a),
                1 => Some(page_b),
                _other => None,
            },
        )
        .expect("the valid prefix frame extracts");
        assert_eq!(prefix_draws, 0..1);

        let positions = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 3.0),
            Vec2::new(2.0, 3.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(12.0, 0.0),
            Vec2::new(12.0, 2.0),
            Vec2::new(10.0, 2.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(22.0, 0.0),
            Vec2::new(21.0, 2.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(32.0, 0.0),
            Vec2::new(31.0, 2.0),
        ];
        let frame = SpinalFrame {
            revision: 2,
            draws: vec![
                crate::runtime::SpinalDraw {
                    page_ordinal: 0,
                    vertices: 0..5,
                    indices: 0..9,
                    color: [1.0; 4],
                },
                crate::runtime::SpinalDraw {
                    page_ordinal: 0,
                    vertices: 5..9,
                    indices: 9..15,
                    color: [1.0; 4],
                },
                crate::runtime::SpinalDraw {
                    page_ordinal: 1,
                    vertices: 9..12,
                    indices: 15..18,
                    color: [1.0; 4],
                },
                crate::runtime::SpinalDraw {
                    page_ordinal: 0,
                    vertices: 12..15,
                    indices: 18..21,
                    color: [1.0; 4],
                },
            ],
            vertices: positions
                .into_iter()
                .map(|position| crate::runtime::SpinalVertex {
                    position,
                    uv: position / 100.0,
                })
                .collect(),
            indices: vec![
                0, 1, 4, 0, 4, 3, 1, 2, 4, 5, 6, 7, 5, 7, 8, 9, 10, 11, 12, 13, 14,
            ],
            issue_points: Vec::new(),
            ready: true,
        };
        let mut appearance = SpinalAppearance::default();
        appearance.set_flip_x(true);
        appearance.set_flip_y(true);
        let transform = GlobalTransform::from_translation(Vec3::new(100.0, 200.0, 3.0));
        let frame_draws = append_extracted_frame_geometry(
            &mut extracted,
            &transform,
            &appearance,
            &frame,
            |ordinal| match ordinal {
                0 => Some(page_a),
                1 => Some(page_b),
                _other => None,
            },
        )
        .expect("the valid arbitrary indexed frame extracts");

        assert_eq!(frame_draws, 1..5);
        assert_eq!(extracted.vertices.len(), 18);
        assert_eq!(extracted.vertices[3].position, Vec3::new(100.0, 200.0, 3.0));
        assert_eq!(extracted.vertices[7].position, Vec3::new(98.0, 197.0, 3.0));
        assert_eq!(
            &extracted.indices[3..],
            [
                3, 4, 7, 3, 7, 6, 4, 5, 7, 8, 9, 10, 8, 10, 11, 12, 13, 14, 15, 16, 17,
            ]
        );
        assert_eq!(
            extracted.draws[frame_draws.clone()]
                .iter()
                .map(|draw| (draw.image, draw.indices.clone()))
                .collect::<Vec<_>>(),
            [
                (page_a, 3..12),
                (page_a, 12..18),
                (page_b, 18..21),
                (page_a, 21..24),
            ]
        );

        let mut batches = Vec::new();
        for draw in &extracted.draws[frame_draws] {
            push_adjacent_batch(
                &mut batches,
                0,
                draw.image,
                draw.indices.start as u32..draw.indices.end as u32,
            );
        }
        assert_eq!(
            batches,
            [
                AdjacentBatch {
                    key: page_a,
                    indices: 3..18,
                },
                AdjacentBatch {
                    key: page_b,
                    indices: 18..21,
                },
                AdjacentBatch {
                    key: page_a,
                    indices: 21..24,
                },
            ]
        );
    }

    #[test]
    fn indexed_extraction_rolls_back_a_partially_appended_invalid_frame() {
        let mut images = Assets::<Image>::default();
        let page = images.add(Image::default()).id();
        let sentinel = ExtractedVertex {
            position: Vec3::new(9.0, 8.0, 7.0),
            uv: Vec2::splat(0.5),
            color: [0.25; 4],
        };
        let mut extracted = ExtractedSpinalFrames {
            vertices: vec![sentinel],
            indices: vec![0],
            draws: vec![ExtractedDraw {
                image: page,
                indices: 0..1,
            }],
            ..Default::default()
        };
        let frame = SpinalFrame {
            revision: 1,
            draws: vec![
                crate::runtime::SpinalDraw {
                    page_ordinal: 0,
                    vertices: 0..3,
                    indices: 0..3,
                    color: [1.0; 4],
                },
                crate::runtime::SpinalDraw {
                    page_ordinal: 1,
                    vertices: 3..6,
                    indices: 3..6,
                    color: [1.0; 4],
                },
            ],
            vertices: [
                Vec2::ZERO,
                Vec2::X,
                Vec2::Y,
                Vec2::splat(2.0),
                Vec2::new(3.0, 2.0),
                Vec2::new(2.0, 3.0),
            ]
            .into_iter()
            .map(|position| crate::runtime::SpinalVertex {
                position,
                uv: Vec2::ZERO,
            })
            .collect(),
            indices: vec![0, 1, 2, 3, 4, 5],
            issue_points: Vec::new(),
            ready: true,
        };

        assert!(
            append_extracted_frame_geometry(
                &mut extracted,
                &GlobalTransform::IDENTITY,
                &SpinalAppearance::default(),
                &frame,
                |ordinal| (ordinal == 0).then_some(page),
            )
            .is_none()
        );
        assert_eq!(extracted.vertices.len(), 1);
        assert_eq!(extracted.vertices[0].position, sentinel.position);
        assert_eq!(extracted.indices, [0]);
        assert_eq!(extracted.draws.len(), 1);
        assert_eq!(extracted.draws[0].image, page);
        assert_eq!(extracted.draws[0].indices, 0..1);
    }

    #[test]
    fn packed_vertex_matches_the_declared_vertex_layout() {
        let vertex = ExtractedVertex {
            position: Vec3::new(1.0, 2.0, 3.0),
            uv: Vec2::new(0.1, 0.2),
            color: [0.9, 0.8, 0.7, 0.6],
        };

        assert_eq!(
            pack_vertex(&vertex),
            [1.0, 2.0, 3.0, 0.1, 0.2, 0.9, 0.8, 0.7, 0.6]
        );
        assert_eq!(gpu_vertex_layout().array_stride, GPU_VERTEX_STRIDE);
    }

    #[test]
    fn reflected_quads_are_not_culled() {
        assert_eq!(spinal_primitive_state().cull_mode, None);
    }

    #[test]
    fn local_facing_is_applied_before_the_entity_transform() {
        let transform = GlobalTransform::from_translation(Vec3::new(10.0, 20.0, 0.0));
        assert_eq!(
            transform_position(&transform, Vec2::new(3.0, 4.0), true, false),
            Some(Vec3::new(7.0, 24.0, 0.0))
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

use std::sync::Arc;

use bevy::{
    asset::{
        Asset, AssetLoader, AssetPath, Handle, LoadContext, ParseAssetPathError,
        ReadAssetBytesError, RenderAssetUsages, UntypedAssetId, VisitAssetDependencies, io::Reader,
    },
    image::{
        CompressedImageFormats, Image, ImageAddressMode, ImageFilterMode, ImageSampler,
        ImageSamplerDescriptor, ImageType, TextureError,
    },
    reflect::TypePath,
};
use serde::{Deserialize, Serialize};
use spinal::{
    AtlasPageId, AtlasPageRef, Diagnostic, IdError, PixelSize, SkeletonAsset, TextureFilter,
    WrapMode,
};
use thiserror::Error;

/// A linked Spinal skeleton and its Bevy-managed atlas page images.
///
/// The value is replaced as one Bevy asset when its skeleton JSON or text
/// atlas reloads successfully. Each page image uses a stable labeled handle,
/// so successful reloads do not invalidate renderer references.
#[derive(Debug, TypePath)]
pub struct SpinalAsset {
    skeleton: Arc<SkeletonAsset>,
    pages: Vec<SpinalAtlasPage>,
}

impl SpinalAsset {
    /// Creates a compound asset from a linked skeleton and page images.
    ///
    /// Pages must match the skeleton's atlas pages exactly in source order and
    /// by authored name. The loader applies the same check, making this
    /// constructor suitable for embedded, generated, and test assets.
    pub fn new(
        skeleton: Arc<SkeletonAsset>,
        mut pages: Vec<SpinalAtlasPage>,
    ) -> Result<Self, SpinalAssetLoaderError> {
        let expected_count = skeleton.atlas_pages().len();
        if pages.len() != expected_count {
            return Err(SpinalAssetLoaderError::PageCountMismatch {
                expected: expected_count,
                actual: pages.len(),
            });
        }

        for (page, expected) in pages.iter_mut().zip(skeleton.atlas_pages()) {
            if page.name.as_ref() != expected.name() {
                return Err(SpinalAssetLoaderError::PageNameMismatch {
                    ordinal: expected.ordinal(),
                    expected: expected.name().into(),
                    actual: page.name.clone(),
                });
            }
            page.ordinal = expected.ordinal();
        }

        Ok(Self { skeleton, pages })
    }

    /// Returns the immutable renderer-independent skeleton.
    #[must_use]
    pub const fn skeleton(&self) -> &Arc<SkeletonAsset> {
        &self.skeleton
    }

    /// Returns the atlas pages in source order.
    #[must_use]
    pub fn pages(&self) -> &[SpinalAtlasPage] {
        &self.pages
    }

    /// Returns one atlas page by source-order position.
    #[must_use]
    pub fn page(&self, ordinal: usize) -> Option<&SpinalAtlasPage> {
        self.pages.get(ordinal)
    }

    /// Returns one atlas page after validating its asset-scoped core ID.
    pub fn page_by_id(&self, id: AtlasPageId) -> Result<&SpinalAtlasPage, IdError> {
        let ordinal = self.skeleton.atlas_page(id)?.ordinal();
        Ok(&self.pages[ordinal])
    }

    /// Returns non-fatal load diagnostics retained by the core asset.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.skeleton.diagnostics()
    }

    /// Returns whether a retained diagnostic changes visible or behavioral
    /// output.
    #[must_use]
    pub fn has_degradations(&self) -> bool {
        self.skeleton.has_degradations()
    }
}

impl VisitAssetDependencies for SpinalAsset {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedAssetId)) {
        for page in &self.pages {
            visit(page.image.id().untyped());
        }
    }
}

impl Asset for SpinalAsset {}

/// One source-order atlas page and its Bevy image handle.
#[derive(Clone, Debug)]
pub struct SpinalAtlasPage {
    ordinal: usize,
    name: Box<str>,
    source_path: Option<AssetPath<'static>>,
    image: Handle<Image>,
}

impl SpinalAtlasPage {
    /// Creates a page for manual [`SpinalAsset`] construction.
    ///
    /// [`SpinalAsset::new`] validates the name and assigns the source-order
    /// position. Manually constructed pages have no filesystem source path.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, image: Handle<Image>) -> Self {
        Self {
            ordinal: 0,
            name: name.into(),
            source_path: None,
            image,
        }
    }

    fn from_loaded(
        name: impl Into<Box<str>>,
        source_path: AssetPath<'static>,
        image: Handle<Image>,
    ) -> Self {
        Self {
            source_path: Some(source_path),
            ..Self::new(name, image)
        }
    }

    /// Returns this page's source-order position.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// Returns the page image name exactly as authored in the atlas.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the resolved source path for a loader-created page.
    ///
    /// Embedded or generated pages created with [`Self::new`] return `None`.
    #[must_use]
    pub fn source_path(&self) -> Option<&AssetPath<'static>> {
        self.source_path.as_ref()
    }

    /// Returns the Bevy image handle used by the renderer.
    #[must_use]
    pub const fn image(&self) -> &Handle<Image> {
        &self.image
    }
}

/// Per-load configuration for [`SpinalAssetLoader`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpinalAssetLoaderSettings {
    /// Optional text-atlas path embedded relative to the skeleton JSON.
    ///
    /// When omitted, `cat.spine.json` and `cat.json` both infer
    /// `cat.atlas`.
    pub atlas_path: Option<String>,
}

/// Bevy loader for a Spine 4.3 JSON export, text atlas, and atlas images.
///
/// The preferred filename is `name.spine.json`. A typed
/// `Handle<SpinalAsset>` may also load an unrenamed `name.json`; the loader
/// deliberately does not claim every `.json` file by extension.
#[derive(Clone, Copy, Debug, Default, TypePath)]
pub struct SpinalAssetLoader;

impl AssetLoader for SpinalAssetLoader {
    type Asset = SpinalAsset;
    type Settings = SpinalAssetLoaderSettings;
    type Error = SpinalAssetLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let skeleton_path = load_context.path().clone_owned();
        let atlas_reference = match settings.atlas_path.as_deref() {
            Some(path) => path.to_owned(),
            None => infer_atlas_reference(&skeleton_path)?,
        };
        let atlas_path = resolve_dependency(&skeleton_path, &atlas_reference, "atlas")?;

        let mut skeleton_json = Vec::new();
        reader.read_to_end(&mut skeleton_json).await?;
        let atlas_text = load_context
            .read_asset_bytes(atlas_path.clone())
            .await
            .map_err(|source| SpinalAssetLoaderError::AtlasRead {
                path: atlas_path.clone(),
                source: Box::new(source),
            })?;
        let skeleton = spinal::load_json(&skeleton_json, &atlas_text)?.into_asset();

        let page_specs = skeleton
            .atlas_pages()
            .map(PageLoadSpec::from)
            .collect::<Vec<_>>();
        let mut pages = Vec::with_capacity(page_specs.len());

        for spec in page_specs {
            let page_path = resolve_dependency(&atlas_path, &spec.name, "atlas page")?;
            let sampler = page_sampler(spec.min_filter, spec.mag_filter, spec.wrap);
            let image_bytes = load_context
                .read_asset_bytes(page_path.clone())
                .await
                .map_err(|source| SpinalAssetLoaderError::PageImageRead {
                    page: spec.name.clone(),
                    path: page_path.clone(),
                    source: Box::new(source),
                })?;
            let extension = page_path
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .ok_or_else(|| SpinalAssetLoaderError::MissingPageImageExtension {
                    page: spec.name.clone(),
                    path: page_path.clone(),
                })?;
            let image_asset = Image::from_buffer(
                &image_bytes,
                ImageType::Extension(extension),
                CompressedImageFormats::NONE,
                true,
                sampler,
                RenderAssetUsages::default(),
            )
            .map_err(|source| SpinalAssetLoaderError::PageImageDecode {
                page: spec.name.clone(),
                path: page_path.clone(),
                source: Box::new(source),
            })?;
            validate_page_image_size(&spec.name, &page_path, spec.declared_size, &image_asset)?;
            let image =
                load_context.add_labeled_asset(format!("page-{}", spec.ordinal), image_asset);
            pages.push(SpinalAtlasPage::from_loaded(spec.name, page_path, image));
        }

        SpinalAsset::new(skeleton, pages)
    }

    fn extensions(&self) -> &[&str] {
        &["spine.json"]
    }
}

/// A failure while constructing or loading a [`SpinalAsset`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SpinalAssetLoaderError {
    /// Skeleton JSON bytes could not be read.
    #[error("could not read Spinal skeleton JSON: {0}")]
    Io(#[from] std::io::Error),

    /// A sibling atlas filename could not be inferred.
    #[error(
        "cannot infer a text atlas for `{path}`; use a `.spine.json` or `.json` skeleton filename, or set `atlas_path`"
    )]
    CannotInferAtlas {
        /// Skeleton asset path that could not be interpreted.
        path: AssetPath<'static>,
    },

    /// An embedded atlas or page reference was not a valid Bevy asset path.
    #[error("invalid {dependency} reference `{reference}` in `{base}`: {source}")]
    InvalidDependencyReference {
        /// Kind of dependency being resolved.
        dependency: &'static str,
        /// Asset containing the embedded reference.
        base: Box<AssetPath<'static>>,
        /// Embedded reference as authored or configured.
        reference: Box<str>,
        /// Bevy path parser failure.
        #[source]
        source: ParseAssetPathError,
    },

    /// A dependency escaped its asset source, selected another source, or
    /// addressed a labeled subasset instead of a file.
    #[error(
        "{dependency} reference `{reference}` in `{base}` resolved to disallowed path `{resolved}`"
    )]
    DisallowedDependencyPath {
        /// Kind of dependency being resolved.
        dependency: &'static str,
        /// Asset containing the embedded reference.
        base: Box<AssetPath<'static>>,
        /// Embedded reference as authored or configured.
        reference: Box<str>,
        /// Rejected resolved path.
        resolved: Box<AssetPath<'static>>,
    },

    /// The text atlas bytes could not be read.
    #[error("could not read Spinal text atlas `{path}`: {source}")]
    AtlasRead {
        /// Resolved atlas path.
        path: AssetPath<'static>,
        /// Bevy asset-reader failure.
        #[source]
        source: Box<ReadAssetBytesError>,
    },

    /// Skeleton JSON or text-atlas data failed core parsing or linking.
    #[error("could not parse and link Spinal export: {0}")]
    Core(#[from] spinal::LoadError),

    /// Atlas page image bytes could not be read.
    #[error("could not read Spinal atlas page `{page}` from `{path}`: {source}")]
    PageImageRead {
        /// Page name exactly as authored in the atlas.
        page: Box<str>,
        /// Resolved page image path.
        path: AssetPath<'static>,
        /// Bevy asset-reader failure.
        #[source]
        source: Box<ReadAssetBytesError>,
    },

    /// An atlas page path had no filename extension for format selection.
    #[error("cannot determine the image format for Spinal atlas page `{page}` at `{path}`")]
    MissingPageImageExtension {
        /// Page name exactly as authored in the atlas.
        page: Box<str>,
        /// Resolved page image path.
        path: AssetPath<'static>,
    },

    /// Bevy could not decode an atlas page image.
    #[error("could not decode Spinal atlas page `{page}` from `{path}`: {source}")]
    PageImageDecode {
        /// Page name exactly as authored in the atlas.
        page: Box<str>,
        /// Resolved page image path.
        path: AssetPath<'static>,
        /// Bevy image decoder failure.
        #[source]
        source: Box<TextureError>,
    },

    /// A decoded atlas page did not match its positive declared pixel size.
    #[error(
        "Spinal atlas page `{page}` at `{path}` declares {expected_width}x{expected_height} pixels but decoded as {actual_width}x{actual_height}"
    )]
    #[non_exhaustive]
    PageImageSizeMismatch {
        /// Page name exactly as authored in the atlas.
        page: Box<str>,
        /// Resolved page image path.
        path: AssetPath<'static>,
        /// Width declared by the text atlas.
        expected_width: u32,
        /// Height declared by the text atlas.
        expected_height: u32,
        /// Width reported by the decoded Bevy image.
        actual_width: u32,
        /// Height reported by the decoded Bevy image.
        actual_height: u32,
    },

    /// Manual page construction supplied the wrong number of pages.
    #[error("Spinal asset requires {expected} atlas pages but received {actual}")]
    PageCountMismatch {
        /// Atlas page count in the linked skeleton.
        expected: usize,
        /// Number of page images supplied by the caller.
        actual: usize,
    },

    /// A manually supplied page name did not match the linked atlas.
    #[error("Spinal atlas page {ordinal} is named `{expected}` but received `{actual}`")]
    PageNameMismatch {
        /// Source-order page position.
        ordinal: usize,
        /// Name retained by the linked skeleton.
        expected: Box<str>,
        /// Name supplied with the image handle.
        actual: Box<str>,
    },
}

#[derive(Clone, Debug)]
struct PageLoadSpec {
    ordinal: usize,
    name: Box<str>,
    declared_size: PixelSize,
    min_filter: TextureFilter,
    mag_filter: TextureFilter,
    wrap: WrapMode,
}

impl From<AtlasPageRef<'_>> for PageLoadSpec {
    fn from(page: AtlasPageRef<'_>) -> Self {
        Self {
            ordinal: page.ordinal(),
            name: Box::<str>::from(page.name()),
            declared_size: page.size(),
            min_filter: page.min_filter(),
            mag_filter: page.mag_filter(),
            wrap: page.wrap(),
        }
    }
}

fn validate_page_image_size(
    page: &str,
    path: &AssetPath<'static>,
    declared_size: PixelSize,
    image: &Image,
) -> Result<(), SpinalAssetLoaderError> {
    if declared_size == PixelSize::default()
        || (declared_size.width() == image.width() && declared_size.height() == image.height())
    {
        return Ok(());
    }

    Err(SpinalAssetLoaderError::PageImageSizeMismatch {
        page: page.into(),
        path: path.clone(),
        expected_width: declared_size.width(),
        expected_height: declared_size.height(),
        actual_width: image.width(),
        actual_height: image.height(),
    })
}

fn infer_atlas_reference(
    skeleton_path: &AssetPath<'static>,
) -> Result<String, SpinalAssetLoaderError> {
    let Some(file_name) = skeleton_path
        .path()
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Err(SpinalAssetLoaderError::CannotInferAtlas {
            path: skeleton_path.clone(),
        });
    };
    let stem = file_name
        .strip_suffix(".spine.json")
        .or_else(|| file_name.strip_suffix(".json"))
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| SpinalAssetLoaderError::CannotInferAtlas {
            path: skeleton_path.clone(),
        })?;
    Ok(format!("{stem}.atlas"))
}

fn resolve_dependency(
    base: &AssetPath<'static>,
    reference: &str,
    dependency: &'static str,
) -> Result<AssetPath<'static>, SpinalAssetLoaderError> {
    let resolved = base.resolve_embed_str(reference).map_err(|source| {
        SpinalAssetLoaderError::InvalidDependencyReference {
            dependency,
            base: Box::new(base.clone()),
            reference: reference.into(),
            source,
        }
    })?;
    let disallowed = resolved.source() != base.source()
        || resolved.is_unapproved()
        || resolved.label().is_some()
        || resolved.path().file_name().is_none();
    if disallowed {
        return Err(SpinalAssetLoaderError::DisallowedDependencyPath {
            dependency,
            base: Box::new(base.clone()),
            reference: reference.into(),
            resolved: Box::new(resolved),
        });
    }
    Ok(resolved)
}

fn page_sampler(
    min_filter: TextureFilter,
    mag_filter: TextureFilter,
    wrap: WrapMode,
) -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_mode(wrap.x()),
        address_mode_v: address_mode(wrap.y()),
        min_filter: within_level_filter(min_filter),
        mag_filter: within_level_filter(mag_filter),
        mipmap_filter: mipmap_filter(min_filter),
        ..Default::default()
    })
}

const fn address_mode(repeat: bool) -> ImageAddressMode {
    if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    }
}

const fn within_level_filter(filter: TextureFilter) -> ImageFilterMode {
    match filter {
        TextureFilter::Linear
        | TextureFilter::MipMap
        | TextureFilter::MipMapLinearNearest
        | TextureFilter::MipMapLinearLinear => ImageFilterMode::Linear,
        TextureFilter::Nearest
        | TextureFilter::MipMapNearestNearest
        | TextureFilter::MipMapNearestLinear
        | TextureFilter::Unknown => ImageFilterMode::Nearest,
        _ => ImageFilterMode::Nearest,
    }
}

const fn mipmap_filter(filter: TextureFilter) -> ImageFilterMode {
    match filter {
        TextureFilter::MipMap
        | TextureFilter::MipMapNearestLinear
        | TextureFilter::MipMapLinearLinear => ImageFilterMode::Linear,
        TextureFilter::Nearest
        | TextureFilter::Linear
        | TextureFilter::MipMapNearestNearest
        | TextureFilter::MipMapLinearNearest
        | TextureFilter::Unknown => ImageFilterMode::Nearest,
        _ => ImageFilterMode::Nearest,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "render")]
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    use bevy::{
        asset::{AssetPath, Handle, VisitAssetDependencies},
        image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler},
    };
    use spinal::{PixelSize, TextureFilter, WrapMode, load_json};

    #[cfg(feature = "render")]
    use super::validate_page_image_size;
    use super::{
        PageLoadSpec, SpinalAsset, SpinalAssetLoaderError, SpinalAssetLoaderSettings,
        SpinalAtlasPage, infer_atlas_reference, page_sampler, resolve_dependency,
    };

    const JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}]
    }"#;

    #[test]
    fn infers_compound_and_plain_json_atlas_names() {
        assert_eq!(
            infer_atlas_reference(&AssetPath::parse("animals/sample.spine.json").into_owned())
                .expect("compound extension is supported"),
            "sample.atlas"
        );
        assert_eq!(
            infer_atlas_reference(&AssetPath::parse("animals/sample.json").into_owned())
                .expect("typed plain JSON load is supported"),
            "sample.atlas"
        );
        assert!(matches!(
            infer_atlas_reference(&AssetPath::parse("animals/sample.bin").into_owned()),
            Err(SpinalAssetLoaderError::CannotInferAtlas { .. })
        ));
    }

    #[test]
    fn embedded_dependencies_are_normalized_but_cannot_escape_or_switch_source() {
        let base = AssetPath::parse("animals/exports/sample.atlas").into_owned();
        assert_eq!(
            resolve_dependency(&base, "../images/sample.png", "atlas page")
                .expect("normalized path stays in the source"),
            AssetPath::parse("animals/images/sample.png")
        );
        assert!(matches!(
            resolve_dependency(&base, "../../../secret.png", "atlas page"),
            Err(SpinalAssetLoaderError::DisallowedDependencyPath { .. })
        ));
        assert!(matches!(
            resolve_dependency(&base, "remote://sample.png", "atlas page"),
            Err(SpinalAssetLoaderError::DisallowedDependencyPath { .. })
        ));
        assert!(matches!(
            resolve_dependency(&base, "sample.png#thumbnail", "atlas page"),
            Err(SpinalAssetLoaderError::DisallowedDependencyPath { .. })
        ));
    }

    #[test]
    fn authored_sampler_maps_filters_and_axis_specific_wrap() {
        let sampler = page_sampler(
            TextureFilter::MipMapLinearNearest,
            TextureFilter::Linear,
            WrapMode::new(true, false),
        );
        let ImageSampler::Descriptor(sampler) = sampler else {
            panic!("authored atlas settings use a descriptor");
        };
        assert_eq!(sampler.min_filter, ImageFilterMode::Linear);
        assert_eq!(sampler.mag_filter, ImageFilterMode::Linear);
        assert_eq!(sampler.mipmap_filter, ImageFilterMode::Nearest);
        assert_eq!(sampler.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(sampler.address_mode_v, ImageAddressMode::ClampToEdge);

        let sampler = page_sampler(
            TextureFilter::MipMapNearestLinear,
            TextureFilter::Nearest,
            WrapMode::new(false, true),
        );
        let ImageSampler::Descriptor(sampler) = sampler else {
            panic!("authored atlas settings use a descriptor");
        };
        assert_eq!(sampler.min_filter, ImageFilterMode::Nearest);
        assert_eq!(sampler.mag_filter, ImageFilterMode::Nearest);
        assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
        assert_eq!(sampler.address_mode_u, ImageAddressMode::ClampToEdge);
        assert_eq!(sampler.address_mode_v, ImageAddressMode::Repeat);
    }

    #[test]
    fn checked_manual_construction_validates_names_and_visits_images() {
        let skeleton = load_json(JSON, b"cat.png\n")
            .expect("fixture is valid")
            .into_asset();
        let image = Handle::<Image>::default();
        let mut asset_ids = Vec::new();
        let asset = SpinalAsset::new(
            skeleton.clone(),
            vec![SpinalAtlasPage::new("cat.png", image.clone())],
        )
        .expect("matching source-order page is accepted");
        asset.visit_dependencies(&mut |id| asset_ids.push(id));

        assert_eq!(asset.skeleton().spine_version(), "4.3.23");
        assert_eq!(asset.page(0).expect("one page").ordinal(), 0);
        assert_eq!(asset_ids, [image.id().untyped()]);
        assert!(matches!(
            SpinalAsset::new(skeleton, vec![SpinalAtlasPage::new("wrong.png", image)]),
            Err(SpinalAssetLoaderError::PageNameMismatch { .. })
        ));
    }

    #[test]
    fn loader_settings_default_to_sibling_atlas_inference() {
        let settings = SpinalAssetLoaderSettings::default();
        assert_eq!(settings.atlas_path, None);
    }

    #[test]
    fn page_load_spec_preserves_declared_atlas_size() {
        let skeleton = load_json(JSON, b"cat.png\n\tsize: 128, 64\n")
            .expect("fixture is valid")
            .into_asset();
        let page = skeleton.atlas_pages().next().expect("one atlas page");
        let spec = PageLoadSpec::from(page);

        assert_eq!(spec.declared_size, PixelSize::new(128, 64));
    }

    #[test]
    #[cfg(feature = "render")]
    fn decoded_page_size_must_match_a_positive_atlas_declaration() {
        let path = AssetPath::parse("cats/cat.png").into_owned();
        let image = test_image(64, 32);
        let error = validate_page_image_size("cat.png", &path, PixelSize::new(128, 64), &image)
            .expect_err("a decoded image with different dimensions must be rejected");

        assert!(matches!(
            error,
            SpinalAssetLoaderError::PageImageSizeMismatch {
                page,
                path: error_path,
                expected_width: 128,
                expected_height: 64,
                actual_width: 64,
                actual_height: 32,
            } if page.as_ref() == "cat.png" && error_path == path
        ));
    }

    #[test]
    #[cfg(feature = "render")]
    fn omitted_or_matching_page_size_accepts_decoded_dimensions() {
        let path = AssetPath::parse("cats/cat.png").into_owned();
        let image = test_image(64, 32);

        validate_page_image_size("cat.png", &path, PixelSize::new(0, 0), &image)
            .expect("an omitted atlas page size uses the decoded image dimensions");
        validate_page_image_size("cat.png", &path, PixelSize::new(64, 32), &image)
            .expect("matching declared and decoded dimensions are accepted");
    }

    #[cfg(feature = "render")]
    fn test_image(width: u32, height: u32) -> Image {
        Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; width as usize * height as usize * 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

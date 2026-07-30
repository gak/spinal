#![no_main]

use libfuzzer_sys::fuzz_target;
use spinal::{Skeleton, load_json};

const SKELETON_JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;

fuzz_target!(|data: &[u8]| {
    if let Ok(report) = load_json(SKELETON_JSON, data) {
        let asset = report.into_asset();
        for page in asset.atlas_pages() {
            let page = asset
                .atlas_page(page.id())
                .expect("loader emitted a valid atlas page ID");
            for region in page.regions() {
                let _region = asset
                    .atlas_region(region.id())
                    .expect("loader emitted a valid atlas region ID");
            }
        }
        let _instance = Skeleton::new(asset);
    }
});

//! Public contract tests for the Stage 1 standalone core foundation.

use spinal::{
    Angle, BoneTransform, Mix, Shear, Skeleton, SkeletonAsset, TARGET_SPINE_VERSION, glam::Vec2,
};

#[test]
fn the_target_wire_contract_is_explicit_without_claiming_conformance() {
    assert_eq!(TARGET_SPINE_VERSION, "4.3.23");
}

#[test]
fn angles_store_finite_radians() {
    let angle = Angle::from_degrees(180.0).expect("180 degrees is finite");
    assert!((angle.as_radians() - core::f32::consts::PI).abs() < 1.0e-6);
    assert!(Angle::from_radians(f32::NAN).is_err());
    assert!(Angle::from_degrees(f32::INFINITY).is_err());
}

#[test]
fn mixes_reject_invalid_values_and_clamp_only_when_asked() {
    assert_eq!(Mix::new(0.25).expect("mix is in range").get(), 0.25);
    assert!(Mix::new(-0.01).is_err());
    assert!(Mix::new(1.01).is_err());
    assert!(Mix::new(f32::NAN).is_err());
    assert_eq!(
        Mix::clamped(1.5)
            .expect("a finite value can be clamped")
            .get(),
        1.0
    );
}

#[test]
fn transforms_default_to_the_identity_pose() {
    let transform = BoneTransform::IDENTITY;
    assert_eq!(transform.translation(), Vec2::ZERO);
    assert_eq!(transform.rotation(), Angle::ZERO);
    assert_eq!(transform.scale(), Vec2::ONE);
    assert_eq!(transform.shear(), Shear::ZERO);
}

#[test]
fn shear_is_unit_safe_on_both_axes() {
    let shear = Shear::from_degrees(15.0, -10.0).expect("both axes are finite");
    assert_eq!(shear.x(), Angle::from_degrees(15.0).expect("finite"));
    assert_eq!(shear.y(), Angle::from_degrees(-10.0).expect("finite"));
    assert!(Shear::from_degrees(f32::NAN, 0.0).is_err());
}

#[test]
fn core_assets_and_instances_are_thread_safe() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SkeletonAsset>();
    assert_send_sync::<Skeleton>();
}

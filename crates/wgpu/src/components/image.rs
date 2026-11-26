//! `image` — GPU-texture or decoded-bytes image content, contain/cover fit,
//! rounded-corner clip (#196).
//!
//! Unlike every other component in this module, `image` has no vector-vs-
//! triangulated lane split: a textured quad's rounded clip is one
//! continuous per-fragment SDF evaluation over the whole quad (see
//! `image.wgsl`), which has no seam to begin with — there is no triangulated
//! fallback to seam-check against.

use crate::theme::ImageStyle;
use crate::types::{CornerRadii, ImageSource, PlacedImage, Rect, Scene, Size};
use mkui_runtime::ImageFit;

/// Compute the `(uv_min, uv_max)` UV rect that reproduces `fit`ting a
/// `natural`-sized image into a `dest`-sized box, so sampling `dest`'s full
/// rect with that UV range draws the fitted result. Pure CPU math, so it is
/// unit-tested directly without any GPU resource.
///
/// - **Cover**: crops. The UV range stays inside `[0, 1]`, narrowed on
///   whichever axis the image overflows the box on.
/// - **Contain**: letterboxes. The UV range extends *outside* `[0, 1]` on
///   whichever axis has slack — `image.wgsl`'s fragment shader renders a
///   sampled UV outside `[0, 1]` as transparent, producing the letterbox
///   bars without a second primitive or an `overflow: hidden`-style clip
///   region.
///
/// Degenerates to the identity UV rect (`[0,0]..[1,1]`, equivalent to CSS
/// `object-fit: fill`) if either size has a non-positive dimension — there
/// is no sensible aspect ratio to fit in that case, and drawing *something*
/// beats panicking or drawing nothing.
pub fn fit_uv(dest: Size, natural: Size, fit: ImageFit) -> ([f32; 2], [f32; 2]) {
    if dest.width <= 0.0 || dest.height <= 0.0 || natural.width <= 0.0 || natural.height <= 0.0 {
        return ([0.0, 0.0], [1.0, 1.0]);
    }
    let dest_aspect = dest.width / dest.height;
    let natural_aspect = natural.width / natural.height;
    let wider = natural_aspect > dest_aspect;

    match fit {
        ImageFit::Cover => {
            let visible_frac = if wider {
                dest_aspect / natural_aspect
            } else {
                natural_aspect / dest_aspect
            };
            let margin = (1.0 - visible_frac) * 0.5;
            if wider {
                ([margin, 0.0], [1.0 - margin, 1.0])
            } else {
                ([0.0, margin], [1.0, 1.0 - margin])
            }
        }
        ImageFit::Contain => {
            let scale = if wider {
                dest_aspect / natural_aspect
            } else {
                natural_aspect / dest_aspect
            };
            let extra = (1.0 / scale - 1.0) * 0.5;
            if wider {
                ([0.0, -extra], [1.0, 1.0 + extra])
            } else {
                ([-extra, 0.0], [1.0 + extra, 1.0])
            }
        }
        // ImageFit is `#[non_exhaustive]`; #196 offers only these two —
        // fall back to the identity rect (CSS `fill`-equivalent) rather
        // than panicking on a future variant.
        _ => ([0.0, 0.0], [1.0, 1.0]),
    }
}

/// Push a placed `Image` primitive: `rect` is the laid-out box, `natural`
/// the image's own intrinsic size (`ImageProps::width`/`height`), `source`
/// already resolved to what this crate's renderer needs (see
/// [`crate::types::ImageSource`]).
pub fn image(
    scene: &mut Scene,
    rect: Rect,
    natural: Size,
    fit: ImageFit,
    source: ImageSource,
    style: ImageStyle,
) {
    let (uv_min, uv_max) = fit_uv(rect.size, natural, fit);
    scene.image(PlacedImage {
        rect,
        uv_min,
        uv_max,
        corner_radii: CornerRadii::all(style.corner_radius),
        source,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crops_a_wider_than_dest_image_symmetrically() {
        // Advisor-verified worked example: 200x100 image in a 100x100 box.
        let (uv_min, uv_max) = fit_uv(
            Size::new(100.0, 100.0),
            Size::new(200.0, 100.0),
            ImageFit::Cover,
        );
        assert!((uv_min[0] - 0.25).abs() < 1e-6);
        assert!((uv_max[0] - 0.75).abs() < 1e-6);
        assert_eq!(uv_min[1], 0.0);
        assert_eq!(uv_max[1], 1.0);
    }

    #[test]
    fn contain_letterboxes_a_wider_than_dest_image_vertically() {
        // Same 200x100-into-100x100 case: full width shown, image occupies
        // the vertically-centered 100x50 — v range extends to [-0.5, 1.5].
        let (uv_min, uv_max) = fit_uv(
            Size::new(100.0, 100.0),
            Size::new(200.0, 100.0),
            ImageFit::Contain,
        );
        assert_eq!(uv_min[0], 0.0);
        assert_eq!(uv_max[0], 1.0);
        assert!((uv_min[1] - -0.5).abs() < 1e-6);
        assert!((uv_max[1] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn cover_crops_a_taller_than_dest_image_vertically() {
        // 100x200 (portrait) into a 100x100 box: width already matches, so
        // covering the box means cropping the overflowing height —
        // symmetric top/bottom, full width visible.
        let (uv_min, uv_max) = fit_uv(
            Size::new(100.0, 100.0),
            Size::new(100.0, 200.0),
            ImageFit::Cover,
        );
        assert_eq!(uv_min[0], 0.0);
        assert_eq!(uv_max[0], 1.0);
        assert!((uv_min[1] - 0.25).abs() < 1e-6);
        assert!((uv_max[1] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn contain_letterboxes_a_taller_than_dest_image_vertically() {
        // Same 100x200-into-100x100 case: full height shown (fits as-is),
        // image occupies the horizontally-centered 50-wide slice — u range
        // extends to [-0.5, 1.5] (the Contain/Cover dual of the test above).
        let (uv_min, uv_max) = fit_uv(
            Size::new(100.0, 100.0),
            Size::new(100.0, 200.0),
            ImageFit::Contain,
        );
        assert_eq!(uv_min[1], 0.0);
        assert_eq!(uv_max[1], 1.0);
        assert!((uv_min[0] - -0.5).abs() < 1e-6);
        assert!((uv_max[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn matching_aspect_ratio_fills_exactly_regardless_of_fit() {
        for fit in [ImageFit::Contain, ImageFit::Cover] {
            let (uv_min, uv_max) = fit_uv(Size::new(80.0, 40.0), Size::new(160.0, 80.0), fit);
            assert_eq!(uv_min, [0.0, 0.0]);
            assert_eq!(uv_max, [1.0, 1.0]);
        }
    }

    #[test]
    fn zero_sized_natural_image_falls_back_to_the_identity_rect() {
        let (uv_min, uv_max) = fit_uv(
            Size::new(100.0, 100.0),
            Size::new(0.0, 0.0),
            ImageFit::Cover,
        );
        assert_eq!(uv_min, [0.0, 0.0]);
        assert_eq!(uv_max, [1.0, 1.0]);
    }

    #[test]
    fn image_pushes_one_placed_image_primitive_with_the_resolved_uv_rect() {
        let mut scene = Scene::new(Size::new(200.0, 200.0));
        image(
            &mut scene,
            Rect::new(crate::types::Point::new(0.0, 0.0), Size::new(100.0, 100.0)),
            Size::new(200.0, 100.0),
            ImageFit::Cover,
            ImageSource::Host(7),
            ImageStyle { corner_radius: 8.0 },
        );
        let placed = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                crate::types::Primitive::Image(placed) => Some(placed),
                _ => None,
            })
            .expect("image() must push a Primitive::Image");
        assert!((placed.uv_min[0] - 0.25).abs() < 1e-6);
        assert_eq!(placed.corner_radii, CornerRadii::all(8.0));
        assert!(matches!(placed.source, ImageSource::Host(7)));
    }
}

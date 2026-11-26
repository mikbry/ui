//! Hand-written parser for the `transform` attribute's function list (#201).
//!
//! Supports exactly `translate`, `scale`, `rotate` (with or without a pivot),
//! `skewX`, `skewY`, and `matrix` — the issue's stated subset
//! ("`transform`'s translate/scale/rotate/skew/matrix"). Any other function
//! name is a typed rejection.
//!
//! [`mkui_vector2d::Affine2`]'s `a b c d tx ty` fields are already defined to
//! match SVG's `matrix(a, b, c, d, e, f)` convention (see that struct's own
//! docs), so this module builds `Affine2` values directly in **SVG space**
//! (y-down) — the y-flip into `mkui-vector2d`'s y-up convention is applied
//! once, centrally, in [`crate::document`] when a shape's accumulated CTM is
//! finalized, not per-function here.

use mkui_vector2d::Affine2;

/// Compose two SVG-space transforms as `outer ∘ inner`: a point is mapped by
/// `inner` first, then by `outer` — i.e. `outer.transform_point(inner.transform_point(p))`.
/// This is the standard 2×3 affine composition; [`Affine2`] has no built-in
/// multiply since the general path lane never needs to compose two of its
/// own transforms.
pub fn compose(outer: Affine2, inner: Affine2) -> Affine2 {
    Affine2 {
        a: outer.a * inner.a + outer.c * inner.b,
        b: outer.b * inner.a + outer.d * inner.b,
        c: outer.a * inner.c + outer.c * inner.d,
        d: outer.b * inner.c + outer.d * inner.d,
        tx: outer.a * inner.tx + outer.c * inner.ty + outer.tx,
        ty: outer.b * inner.tx + outer.d * inner.ty + outer.ty,
    }
}

/// Parse a full `transform="fn(args) fn(args) ..."` attribute value into one
/// composed SVG-space [`Affine2`]. `"A B C"` composes as `A ∘ B ∘ C` (`C`
/// applied first), matching the SVG spec's "equivalent to a single matrix"
/// rule for a transform list. An empty (or whitespace-only) value is the
/// identity transform, not a rejection — SVG explicitly allows
/// `transform=""`. A comma is an optional separator between two functions,
/// same as between two numbers in one function's argument list.
pub fn parse_transform_list(s: &str) -> Option<Affine2> {
    let mut rest = s.trim();
    let mut acc = Affine2::IDENTITY;
    while !rest.is_empty() {
        let (m, tail) = parse_one_function(rest)?;
        acc = compose(acc, m);
        rest = tail.trim_start();
        if let Some(after_comma) = rest.strip_prefix(',') {
            rest = after_comma.trim_start();
        }
    }
    Some(acc)
}

fn parse_one_function(s: &str) -> Option<(Affine2, &str)> {
    let open = s.find('(')?;
    let name = s[..open].trim_end();
    let close_rel = s[open..].find(')')?;
    let close = open + close_rel;
    let args_str = &s[open + 1..close];
    let args = crate::number::parse_number_list(args_str)?;
    let matrix = match name {
        "translate" => match args.as_slice() {
            [tx] => Affine2 {
                tx: *tx,
                ..Affine2::IDENTITY
            },
            [tx, ty] => Affine2 {
                tx: *tx,
                ty: *ty,
                ..Affine2::IDENTITY
            },
            _ => return None,
        },
        "scale" => match args.as_slice() {
            [s] => Affine2 {
                a: *s,
                d: *s,
                ..Affine2::IDENTITY
            },
            [sx, sy] => Affine2 {
                a: *sx,
                d: *sy,
                ..Affine2::IDENTITY
            },
            _ => return None,
        },
        "rotate" => match args.as_slice() {
            [deg] => rotate(*deg),
            [deg, cx, cy] => {
                let t1 = Affine2 {
                    tx: *cx,
                    ty: *cy,
                    ..Affine2::IDENTITY
                };
                let r = rotate(*deg);
                let t2 = Affine2 {
                    tx: -*cx,
                    ty: -*cy,
                    ..Affine2::IDENTITY
                };
                compose(compose(t1, r), t2)
            }
            _ => return None,
        },
        "skewX" => match args.as_slice() {
            [deg] => Affine2 {
                c: deg.to_radians().tan(),
                ..Affine2::IDENTITY
            },
            _ => return None,
        },
        "skewY" => match args.as_slice() {
            [deg] => Affine2 {
                b: deg.to_radians().tan(),
                ..Affine2::IDENTITY
            },
            _ => return None,
        },
        "matrix" => match args.as_slice() {
            [a, b, c, d, tx, ty] => Affine2 {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                tx: *tx,
                ty: *ty,
            },
            _ => return None,
        },
        _ => return None,
    };
    Some((matrix, &s[close + 1..]))
}

fn rotate(deg: f32) -> Affine2 {
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    Affine2 {
        a: cos,
        b: sin,
        c: -sin,
        d: cos,
        tx: 0.0,
        ty: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_vector2d::Vec2;

    #[test]
    fn translate_alone() {
        let m = parse_transform_list("translate(10, 20)").unwrap();
        assert_eq!(m.transform_point(Vec2::ZERO), Vec2::new(10.0, 20.0));
    }

    #[test]
    fn scale_alone() {
        let m = parse_transform_list("scale(2)").unwrap();
        assert_eq!(m.transform_point(Vec2::new(3.0, 4.0)), Vec2::new(6.0, 8.0));
    }

    #[test]
    fn rotate_90_moves_x_axis_to_y_axis() {
        let m = parse_transform_list("rotate(90)").unwrap();
        let p = m.transform_point(Vec2::new(1.0, 0.0));
        assert!((p.x - 0.0).abs() < 1e-5);
        assert!((p.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rotate_about_pivot() {
        let m = parse_transform_list("rotate(180, 10, 10)").unwrap();
        let p = m.transform_point(Vec2::new(10.0, 0.0));
        assert!((p.x - 10.0).abs() < 1e-4);
        assert!((p.y - 20.0).abs() < 1e-4);
    }

    #[test]
    fn matrix_is_passed_through_directly() {
        let m = parse_transform_list("matrix(1,0,0,1,5,6)").unwrap();
        assert_eq!(m.transform_point(Vec2::ZERO), Vec2::new(5.0, 6.0));
    }

    #[test]
    fn list_composes_left_to_right_as_applied_right_to_left() {
        // translate then scale: the point is scaled first, then translated.
        let m = parse_transform_list("translate(10, 0) scale(2)").unwrap();
        assert_eq!(m.transform_point(Vec2::new(1.0, 0.0)), Vec2::new(12.0, 0.0));
    }

    #[test]
    fn unknown_function_is_rejected() {
        assert_eq!(parse_transform_list("shear(1)"), None);
    }

    #[test]
    fn wrong_arity_is_rejected() {
        assert_eq!(parse_transform_list("translate(1,2,3)"), None);
        assert_eq!(parse_transform_list("matrix(1,2,3)"), None);
    }

    #[test]
    fn empty_value_is_the_identity() {
        assert_eq!(parse_transform_list(""), Some(Affine2::IDENTITY));
        assert_eq!(parse_transform_list("   "), Some(Affine2::IDENTITY));
    }

    #[test]
    fn comma_between_functions_is_an_optional_separator() {
        let with_comma = parse_transform_list("translate(10, 0), scale(2)").unwrap();
        let without_comma = parse_transform_list("translate(10, 0) scale(2)").unwrap();
        assert_eq!(with_comma, without_comma);
    }

    #[test]
    fn skew_builds_expected_shear() {
        let m = parse_transform_list("skewX(45)").unwrap();
        let p = m.transform_point(Vec2::new(0.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-4);
    }
}

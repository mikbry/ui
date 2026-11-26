//! Hand-written parser for the `d` attribute's path-data grammar (#201).
//!
//! Covers exactly `M/m L/l H/h V/v C/c S/s Q/q T/t A/a Z/z`: implicit command
//! repetition (extra coordinate groups after a command letter reuse it,
//! with `M`/`m` repeating as `L`/`l`), the `S`/`T` smooth-reflection rule,
//! and elliptical arcs (lowered to cubics by [`crate::arc`]). Output is a
//! flat [`mkui_vector2d::PathCommand`] list in the **same SVG (y-down)
//! space** the numbers were authored in — see [`crate::transform`] for why
//! the y-flip is deferred to [`crate::document`].

use mkui_vector2d::{PathCommand, Vec2};

use crate::arc::arc_to_cubics;
use crate::number::NumberScanner;

/// Parse a full `d` attribute value into a command list. `None` means the
/// value does not match the grammar at all (caller turns this into a typed
/// [`crate::SvgError::InvalidValue`]); an empty `d=""` yields `Some(vec![])`.
pub fn parse_path_data(d: &str) -> Option<Vec<PathCommand>> {
    let mut sc = NumberScanner::new(d);
    let mut out = Vec::new();

    let mut cur = Vec2::ZERO;
    let mut start = Vec2::ZERO;
    let mut active: Option<u8> = None;
    // Reflection memory for S/s and T/t — cleared whenever an unrelated
    // command intervenes (spec: "assumed to be the current point" then).
    let mut prev_cubic_ctrl2: Option<Vec2> = None;
    let mut prev_quad_ctrl: Option<Vec2> = None;

    loop {
        sc.skip_separators();
        if sc.is_at_end() {
            break;
        }
        let letter = match peek_letter(&sc) {
            Some(l) => {
                sc.bump();
                active = Some(l);
                l
            }
            None => match active {
                Some(b'Z') | Some(b'z') => return None, // Z never repeats implicitly
                Some(prev) => {
                    let implicit = implicit_repeat_letter(prev);
                    active = Some(implicit);
                    implicit
                }
                None => return None, // numbers before any command letter
            },
        };
        // The very first command must be a moveto (SVG grammar).
        if out.is_empty() && !matches!(letter, b'M' | b'm') {
            return None;
        }

        let relative = letter.is_ascii_lowercase();
        let resolve = |p: Vec2, dx: f32, dy: f32| -> Vec2 {
            if relative {
                Vec2::new(p.x + dx, p.y + dy)
            } else {
                Vec2::new(dx, dy)
            }
        };

        match letter.to_ascii_uppercase() {
            b'M' => {
                let (x, y) = (sc.read_number()?, sc.read_number()?);
                cur = resolve(cur, x, y);
                start = cur;
                out.push(PathCommand::MoveTo(cur));
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            b'L' => {
                let (x, y) = (sc.read_number()?, sc.read_number()?);
                cur = resolve(cur, x, y);
                out.push(PathCommand::LineTo(cur));
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            b'H' => {
                let x = sc.read_number()?;
                cur = Vec2::new(if relative { cur.x + x } else { x }, cur.y);
                out.push(PathCommand::LineTo(cur));
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            b'V' => {
                let y = sc.read_number()?;
                cur = Vec2::new(cur.x, if relative { cur.y + y } else { y });
                out.push(PathCommand::LineTo(cur));
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            b'C' => {
                let x1 = sc.read_number()?;
                let y1 = sc.read_number()?;
                let x2 = sc.read_number()?;
                let y2 = sc.read_number()?;
                let x = sc.read_number()?;
                let y = sc.read_number()?;
                let control1 = resolve(cur, x1, y1);
                let control2 = resolve(cur, x2, y2);
                let to = resolve(cur, x, y);
                out.push(PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                });
                cur = to;
                prev_cubic_ctrl2 = Some(control2);
                prev_quad_ctrl = None;
            }
            b'S' => {
                let x2 = sc.read_number()?;
                let y2 = sc.read_number()?;
                let x = sc.read_number()?;
                let y = sc.read_number()?;
                let control1 = match prev_cubic_ctrl2 {
                    Some(p) => Vec2::new(2.0 * cur.x - p.x, 2.0 * cur.y - p.y),
                    None => cur,
                };
                let control2 = resolve(cur, x2, y2);
                let to = resolve(cur, x, y);
                out.push(PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                });
                cur = to;
                prev_cubic_ctrl2 = Some(control2);
                prev_quad_ctrl = None;
            }
            b'Q' => {
                let x1 = sc.read_number()?;
                let y1 = sc.read_number()?;
                let x = sc.read_number()?;
                let y = sc.read_number()?;
                let control = resolve(cur, x1, y1);
                let to = resolve(cur, x, y);
                out.push(PathCommand::QuadTo { control, to });
                cur = to;
                prev_quad_ctrl = Some(control);
                prev_cubic_ctrl2 = None;
            }
            b'T' => {
                let x = sc.read_number()?;
                let y = sc.read_number()?;
                let control = match prev_quad_ctrl {
                    Some(p) => Vec2::new(2.0 * cur.x - p.x, 2.0 * cur.y - p.y),
                    None => cur,
                };
                let to = resolve(cur, x, y);
                out.push(PathCommand::QuadTo { control, to });
                cur = to;
                prev_quad_ctrl = Some(control);
                prev_cubic_ctrl2 = None;
            }
            b'A' => {
                let rx = sc.read_number()?;
                let ry = sc.read_number()?;
                let rot = sc.read_number()?;
                let large_arc = sc.read_flag()?;
                let sweep = sc.read_flag()?;
                let x = sc.read_number()?;
                let y = sc.read_number()?;
                let to = resolve(cur, x, y);
                if to == cur {
                    // Spec F.6.2: identical endpoints omit the arc entirely.
                } else {
                    // `None` means this arc's radius-to-chord ratio is too
                    // extreme to represent safely (see
                    // `arc::MAX_RADIUS_TO_CHORD_RATIO`) — the `?` propagates
                    // that out as the same typed `d`-grammar rejection any
                    // other malformed path data gets.
                    for (c1, c2, end) in arc_to_cubics(cur, rx, ry, rot, large_arc, sweep, to)? {
                        out.push(PathCommand::CubicTo {
                            control1: c1,
                            control2: c2,
                            to: end,
                        });
                    }
                }
                cur = to;
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            b'Z' => {
                out.push(PathCommand::Close);
                cur = start;
                prev_cubic_ctrl2 = None;
                prev_quad_ctrl = None;
            }
            _ => return None,
        }
    }

    Some(out)
}

/// `M`/`m` repeats implicitly as `L`/`l`; every other command repeats as
/// itself (including `A`/`a`, which the SVG grammar does allow to repeat).
fn implicit_repeat_letter(letter: u8) -> u8 {
    match letter {
        b'M' => b'L',
        b'm' => b'l',
        other => other,
    }
}

fn peek_letter(sc: &NumberScanner<'_>) -> Option<u8> {
    sc.peek_byte().filter(|b| b.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(cmds: &[PathCommand]) -> Vec<(f32, f32)> {
        cmds.iter()
            .filter_map(|c| match c {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => Some((p.x, p.y)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn triangle_with_close() {
        let cmds = parse_path_data("M0,0 L10,0 L0,10 Z").unwrap();
        assert_eq!(moves(&cmds), vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
        assert!(matches!(cmds.last(), Some(PathCommand::Close)));
    }

    #[test]
    fn implicit_lineto_repeats_after_moveto() {
        let cmds = parse_path_data("M0,0 10,0 0,10").unwrap();
        assert_eq!(moves(&cmds), vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
    }

    #[test]
    fn relative_commands_accumulate_from_current_point() {
        let cmds = parse_path_data("m10,10 l5,0 l0,5").unwrap();
        assert_eq!(moves(&cmds), vec![(10.0, 10.0), (15.0, 10.0), (15.0, 15.0)]);
    }

    #[test]
    fn horizontal_and_vertical_shorthands() {
        let cmds = parse_path_data("M0,0 H10 V10").unwrap();
        assert_eq!(moves(&cmds), vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
    }

    #[test]
    fn packed_numbers_with_no_separators() {
        // "1.5.5" => 1.5, 0.5; "10-5" => 10, -5.
        let cmds = parse_path_data("M0,0L1.5.5 10-5").unwrap();
        let pts = moves(&cmds);
        assert_eq!(pts[1], (1.5, 0.5));
        assert_eq!(pts[2], (10.0, -5.0));
    }

    #[test]
    fn s_reflects_previous_cubic_control_point() {
        // cmds[0] = MoveTo, cmds[1] = the C, cmds[2] = the S.
        let cmds = parse_path_data("M0,0 C0,10 10,10 10,0 S20,-10 20,0").unwrap();
        if let PathCommand::CubicTo { control1, .. } = &cmds[2] {
            // Reflection of the C's control2 (10,10) about the current point
            // (10,0) is (10,-10).
            assert!((control1.x - 10.0).abs() < 1e-5);
            assert!((control1.y - (-10.0)).abs() < 1e-5);
        } else {
            panic!("expected a CubicTo");
        }
    }

    #[test]
    fn t_reflects_previous_quad_control_point() {
        let cmds = parse_path_data("M0,0 Q0,10 10,10 T20,10").unwrap();
        if let PathCommand::QuadTo { control, .. } = &cmds[2] {
            // Reflection of (0,10) about (10,10) is (20,10).
            assert!((control.x - 20.0).abs() < 1e-5);
            assert!((control.y - 10.0).abs() < 1e-5);
        } else {
            panic!("expected a QuadTo");
        }
    }

    #[test]
    fn s_with_no_preceding_curve_uses_current_point() {
        let cmds = parse_path_data("M0,0 S10,10 20,0").unwrap();
        if let PathCommand::CubicTo { control1, .. } = &cmds[1] {
            assert_eq!(*control1, Vec2::ZERO);
        } else {
            panic!("expected a CubicTo");
        }
    }

    #[test]
    fn arc_lowers_to_at_least_one_cubic() {
        let cmds = parse_path_data("M-10,0 A10,10 0 0 1 10,0").unwrap();
        assert!(cmds
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. })));
    }

    #[test]
    fn a_moderately_large_radius_arc_is_followed_by_an_exactly_vertical_line() {
        // Regression (review round 2): an arc whose radius is large
        // relative to its chord used to miss its endpoint by several
        // units, so the following `L10,10` came out diagonal instead of
        // vertical. Radius 1e4 over a 10-unit chord (ratio 1e3) is
        // comfortably inside `arc::MAX_RADIUS_TO_CHORD_RATIO` (review
        // round 4) — a case this must still get right, not reject.
        let cmds = parse_path_data("M0,0 A10000 10000 0 0 1 10,0 L10,10").unwrap();
        let PathCommand::LineTo(final_point) = *cmds.last().unwrap() else {
            panic!("expected the trailing command to be the L10,10 LineTo");
        };
        assert_eq!(final_point, Vec2::new(10.0, 10.0));
        // The point the arc actually landed on, immediately before that
        // final line, must be exactly (10, 0) — not merely close — or the
        // "vertical" claim above is checking the wrong thing.
        let arc_landing = cmds
            .iter()
            .rev()
            .nth(1)
            .expect("at least an arc cubic precedes the final LineTo");
        let landed_at = match *arc_landing {
            PathCommand::CubicTo { to, .. } => to,
            other => panic!("expected the arc to lower to a CubicTo, got {other:?}"),
        };
        assert_eq!(landed_at, Vec2::new(10.0, 0.0));
    }

    #[test]
    fn an_extreme_radius_to_chord_ratio_rejects_the_whole_d_attribute() {
        // `arc::arc_to_cubics` returning `None` for an ill-conditioned
        // radius-to-chord ratio (review round 4) must propagate out of
        // `parse_path_data` as a whole-attribute rejection — not panic,
        // not silently drop just the arc segment.
        assert_eq!(parse_path_data("M0,0 A1e18,1e18 0 0 1 10,0"), None);
    }

    #[test]
    fn arc_with_coincident_endpoints_is_omitted() {
        let cmds = parse_path_data("M5,5 A10,10 0 0 1 5,5 Z").unwrap();
        assert_eq!(cmds.len(), 2); // MoveTo + Close, no cubic emitted.
    }

    #[test]
    fn compacted_arc_flags_parse_correctly() {
        // No separators between the radii/rotation/flags/coords at all:
        // rx=10 ry=10 rot=0 large-arc=0 sweep=0 x=10 y=10.
        let cmds = parse_path_data("M10,0A10 10 0 0010 10").unwrap();
        assert!(cmds
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo { .. })));
        match cmds.last() {
            Some(PathCommand::CubicTo { to, .. }) => {
                assert!((to.x - 10.0).abs() < 1e-4);
                assert!((to.y - 10.0).abs() < 1e-4);
            }
            other => {
                panic!("expected the arc to lower to a CubicTo ending at (10, 10), got {other:?}")
            }
        }
    }

    #[test]
    fn z_followed_by_relative_moveto_starts_a_new_subpath() {
        let cmds = parse_path_data("M0,0 L10,0 Z m5,5 l1,1").unwrap();
        // Second MoveTo is relative to the Z-reset point (0,0), i.e. (5,5).
        let second_move = cmds
            .iter()
            .filter_map(|c| match c {
                PathCommand::MoveTo(p) => Some(*p),
                _ => None,
            })
            .nth(1)
            .unwrap();
        assert_eq!(second_move, Vec2::new(5.0, 5.0));
    }

    #[test]
    fn z_does_not_implicitly_repeat() {
        assert_eq!(parse_path_data("M0,0 L10,0 Z 5,5"), None);
    }

    #[test]
    fn leading_numbers_without_a_command_are_rejected() {
        assert_eq!(parse_path_data("0,0 L10,0"), None);
    }

    #[test]
    fn unknown_command_letter_is_rejected() {
        assert_eq!(parse_path_data("M0,0 X10,0"), None);
    }

    #[test]
    fn empty_d_is_an_empty_command_list() {
        assert_eq!(parse_path_data(""), Some(vec![]));
    }
}

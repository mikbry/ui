//! Minimal column-major 4x4 matrix math for the host's own 3D pass.
//!
//! The example is a "host" standing in for a real game/engine — it is not
//! part of mkui's own render stack, so it hand-rolls just enough matrix math
//! to place and spin a cube rather than pulling in a general-purpose linear
//! algebra crate for one rotating mesh. `Mat4` is column-major (four
//! columns of four `f32`s), matching WGSL's `mat4x4<f32>` uniform layout
//! byte-for-byte — no transpose needed between here and `cube.wgsl`.

pub type Mat4 = [[f32; 4]; 4];

/// Column-major 4x4 multiply: `a * b`, applied to a column vector as `a * (b * v)`.
pub fn mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

/// Right-handed perspective projection into wgpu's `0..1` NDC depth range
/// (unlike OpenGL's `-1..1`), looking down `-Z` in view space.
pub fn perspective(fovy_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy_radians / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, (near * far) / (near - far), 0.0],
    ]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt().max(f32::EPSILON);
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Right-handed view matrix for a camera at `eye` looking at `target`.
pub fn look_at_rh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize3(sub3(target, eye)); // forward
    let s = normalize3(cross3(f, up)); // right
    let u = cross3(s, f); // true up
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot3(s, eye), -dot3(u, eye), dot3(f, eye), 1.0],
    ]
}

/// Rotation about the Y axis by `angle_radians`.
pub fn rotation_y(angle_radians: f32) -> Mat4 {
    let (s, c) = angle_radians.sin_cos();
    [
        [c, 0.0, -s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [s, 0.0, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Mat4 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    #[test]
    fn mul_by_identity_is_a_no_op() {
        let m = rotation_y(0.7);
        assert_eq!(mul(m, IDENTITY), m);
        assert_eq!(mul(IDENTITY, m), m);
    }

    #[test]
    fn rotation_y_by_zero_is_identity() {
        let m = rotation_y(0.0);
        for col in 0..4 {
            for row in 0..4 {
                assert!((m[col][row] - IDENTITY[col][row]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn rotation_y_by_half_turn_flips_x_and_z() {
        let m = rotation_y(std::f32::consts::PI);
        // A point on +X rotates to -X (within floating-point tolerance).
        let p = [1.0, 0.0, 0.0, 1.0];
        let x = m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0] * p[3];
        assert!((x + 1.0).abs() < 1e-5);
    }

    #[test]
    fn perspective_maps_near_plane_to_ndc_zero_depth() {
        // wgpu's NDC depth range is 0..1 (not OpenGL's -1..1): a point at
        // z = -near in view space must land at clip-space depth 0 after the
        // perspective divide.
        let proj = perspective(std::f32::consts::FRAC_PI_4, 1.0, 1.0, 100.0);
        let view_point = [0.0, 0.0, -1.0, 1.0];
        let clip_z = proj[0][2] * view_point[0]
            + proj[1][2] * view_point[1]
            + proj[2][2] * view_point[2]
            + proj[3][2] * view_point[3];
        let clip_w = proj[0][3] * view_point[0]
            + proj[1][3] * view_point[1]
            + proj[2][3] * view_point[2]
            + proj[3][3] * view_point[3];
        assert!((clip_z / clip_w).abs() < 1e-5);
    }

    #[test]
    fn look_at_rh_places_eye_at_origin_in_view_space() {
        let view = look_at_rh([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        // Transforming the eye position itself must land at the view-space
        // origin.
        let eye = [0.0, 0.0, 5.0, 1.0];
        let x =
            view[0][0] * eye[0] + view[1][0] * eye[1] + view[2][0] * eye[2] + view[3][0] * eye[3];
        let y =
            view[0][1] * eye[0] + view[1][1] * eye[1] + view[2][1] * eye[2] + view[3][1] * eye[3];
        let z =
            view[0][2] * eye[0] + view[1][2] * eye[1] + view[2][2] * eye[2] + view[3][2] * eye[3];
        assert!(x.abs() < 1e-5);
        assert!(y.abs() < 1e-5);
        assert!(z.abs() < 1e-5);
    }
}

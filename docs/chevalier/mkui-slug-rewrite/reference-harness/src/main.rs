// Standalone execution harness for Eric Lengyel's pinned Slug shaders.
//
// The shader port is independently derived from upstream commit be3c13e.
// This program deliberately has no dependency on any mkui crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::{Context, Result, bail, ensure};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const BASE_EM_PIXELS: f32 = 96.0;
const BASE_CANVAS_PIXELS: f32 = 128.0;
const BASE_PADDING_PIXELS: f32 = 16.0;
const BAND_COUNT: usize = 8;
const BAND_EPSILON_EM: f32 = 1.0 / 1024.0;
const CURVE_TEXTURE_WIDTH: u32 = 4096;
const BAND_TEXTURE_WIDTH: u32 = 4096;
const GLYPH_REVISION: u32 = 1;
const GLYPH_NAMES: [&str; 8] = ["H", "A", "V", "M", "g", "o", "plus", "pipe"];

#[derive(Debug)]
struct Args {
    dpi: f32,
    glyph: String,
    output: PathBuf,
    shader: PathBuf,
    write_fixtures: Option<PathBuf>,
    compare: Option<(PathBuf, PathBuf, u8)>,
}

fn parse_args() -> Result<Args> {
    let mut dpi = None;
    let mut glyph = None;
    let mut output = None;
    let mut shader = PathBuf::from("src/reference.wgsl");
    let mut write_fixtures = None;
    let mut compare_paths = None;
    let mut threshold = 8u8;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dpi" => {
                dpi = Some(
                    args.next()
                        .context("--dpi needs a value")?
                        .parse::<f32>()
                        .context("--dpi must be a positive number")?,
                );
            }
            "--glyph" => glyph = Some(args.next().context("--glyph needs a name")?),
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().context("--output needs a PNG path")?,
                ));
            }
            "--shader" => {
                shader = PathBuf::from(args.next().context("--shader needs a WGSL path")?);
            }
            "--write-fixtures" => {
                write_fixtures = Some(PathBuf::from(
                    args.next().context("--write-fixtures needs a directory")?,
                ));
            }
            "--compare" => {
                compare_paths = Some((
                    PathBuf::from(args.next().context("--compare needs a known-good PNG")?),
                    PathBuf::from(args.next().context("--compare needs a mutant PNG")?),
                ));
            }
            "--threshold" => {
                threshold = args
                    .next()
                    .context("--threshold needs an integer from 0 through 255")?
                    .parse::<u8>()
                    .context("--threshold must be an integer from 0 through 255")?;
            }
            "--help" | "-h" => {
                println!(
                    "mkui-slug-reference-harness\n\n\
                     Render:\n  \
                     cargo run -- --dpi <1|1.5|2> --glyph <{}> --output <path.png> \
                     [--shader <path.wgsl>]\n\n\
                     Fixture maintenance:\n  \
                     cargo run -- --write-fixtures glyphs\n\n\
                     Sensitivity comparison:\n  \
                     cargo run -- --compare <known-good.png> <mutant.png> --threshold <0..255>",
                    GLYPH_NAMES.join("|")
                );
                std::process::exit(0);
            }
            unknown => bail!("unknown argument: {unknown}"),
        }
    }

    if let Some(dir) = write_fixtures {
        return Ok(Args {
            dpi: 1.0,
            glyph: "H".to_owned(),
            output: PathBuf::new(),
            shader,
            write_fixtures: Some(dir),
            compare: None,
        });
    }

    if let Some((known_good, mutant)) = compare_paths {
        return Ok(Args {
            dpi: 1.0,
            glyph: "H".to_owned(),
            output: PathBuf::new(),
            shader,
            write_fixtures: None,
            compare: Some((known_good, mutant, threshold)),
        });
    }

    let dpi = dpi.context("missing --dpi")?;
    ensure!(dpi.is_finite() && dpi > 0.0, "--dpi must be positive");
    let glyph = glyph.context("missing --glyph")?;
    ensure!(
        GLYPH_NAMES.contains(&glyph.as_str()),
        "unknown glyph {glyph:?}; expected one of {}",
        GLYPH_NAMES.join(", ")
    );
    let output = output.context("missing --output")?;
    ensure!(
        output.extension().and_then(|value| value.to_str()) == Some("png"),
        "--output must end in .png"
    );

    Ok(Args {
        dpi,
        glyph,
        output,
        shader,
        write_fixtures: None,
        compare: None,
    })
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
struct Curve {
    p0: Point,
    p1: Point,
    p2: Point,
}

impl Curve {
    fn line(p0: Point, p2: Point) -> Self {
        Self { p0, p1: p2, p2 }
    }

    fn x_extent(self) -> (f32, f32) {
        (
            self.p0.x.min(self.p1.x).min(self.p2.x),
            self.p0.x.max(self.p1.x).max(self.p2.x),
        )
    }

    fn y_extent(self) -> (f32, f32) {
        (
            self.p0.y.min(self.p1.y).min(self.p2.y),
            self.p0.y.max(self.p1.y).max(self.p2.y),
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    x_min: f32,
    y_min: f32,
    x_max: f32,
    y_max: f32,
}

#[derive(Clone, Copy, Debug)]
struct Band {
    lower: f32,
    upper: f32,
    first_curve: u32,
    curve_count: u32,
}

#[derive(Debug)]
struct Glyph {
    revision: u32,
    bounds: Bounds,
    curves: Vec<Curve>,
    horizontal_bands: Vec<Band>,
    horizontal_curve_indices: Vec<u32>,
    vertical_bands: Vec<Band>,
    vertical_curve_indices: Vec<u32>,
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32> {
    let end = *cursor + 4;
    let chunk = bytes
        .get(*cursor..end)
        .context("truncated SlugGlyph u32 field")?;
    *cursor = end;
    Ok(u32::from_le_bytes(chunk.try_into().unwrap()))
}

fn read_f32(bytes: &[u8], cursor: &mut usize) -> Result<f32> {
    Ok(f32::from_bits(read_u32(bytes, cursor)?))
}

fn read_band_table(bytes: &[u8], cursor: &mut usize) -> Result<(Vec<Band>, Vec<u32>)> {
    let band_count = usize::try_from(read_u32(bytes, cursor)?)?;
    ensure!(band_count > 0, "a glyph must have at least one band");
    let mut bands = Vec::with_capacity(band_count);
    for _ in 0..band_count {
        bands.push(Band {
            lower: read_f32(bytes, cursor)?,
            upper: read_f32(bytes, cursor)?,
            first_curve: read_u32(bytes, cursor)?,
            curve_count: read_u32(bytes, cursor)?,
        });
    }
    let index_count = usize::try_from(read_u32(bytes, cursor)?)?;
    let mut indices = Vec::with_capacity(index_count);
    for _ in 0..index_count {
        indices.push(read_u32(bytes, cursor)?);
    }
    Ok((bands, indices))
}

fn read_glyph(path: &Path) -> Result<Glyph> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut cursor = 0;
    let revision = read_u32(&bytes, &mut cursor)?;
    let bounds = Bounds {
        x_min: read_f32(&bytes, &mut cursor)?,
        y_min: read_f32(&bytes, &mut cursor)?,
        x_max: read_f32(&bytes, &mut cursor)?,
        y_max: read_f32(&bytes, &mut cursor)?,
    };
    let curve_count = usize::try_from(read_u32(&bytes, &mut cursor)?)?;
    ensure!(curve_count > 0, "glyph has no curves");
    let mut curves = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        curves.push(Curve {
            p0: Point::new(
                read_f32(&bytes, &mut cursor)?,
                read_f32(&bytes, &mut cursor)?,
            ),
            p1: Point::new(
                read_f32(&bytes, &mut cursor)?,
                read_f32(&bytes, &mut cursor)?,
            ),
            p2: Point::new(
                read_f32(&bytes, &mut cursor)?,
                read_f32(&bytes, &mut cursor)?,
            ),
        });
    }
    let (horizontal_bands, horizontal_curve_indices) = read_band_table(&bytes, &mut cursor)?;
    let (vertical_bands, vertical_curve_indices) = read_band_table(&bytes, &mut cursor)?;
    ensure!(cursor == bytes.len(), "trailing data in {}", path.display());
    ensure!(
        horizontal_bands.len() <= 256 && vertical_bands.len() <= 256,
        "the pinned shader stores band maxima in eight bits"
    );
    for &index in horizontal_curve_indices
        .iter()
        .chain(&vertical_curve_indices)
    {
        ensure!(
            usize::try_from(index)? < curves.len(),
            "band references missing curve {index}"
        );
    }
    Ok(Glyph {
        revision,
        bounds,
        curves,
        horizontal_bands,
        horizontal_curve_indices,
        vertical_bands,
        vertical_curve_indices,
    })
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(out: &mut Vec<u8>, value: f32) {
    push_u32(out, value.to_bits());
}

fn write_band_table(out: &mut Vec<u8>, bands: &[Band], indices: &[u32]) {
    push_u32(out, bands.len() as u32);
    for band in bands {
        push_f32(out, band.lower);
        push_f32(out, band.upper);
        push_u32(out, band.first_curve);
        push_u32(out, band.curve_count);
    }
    push_u32(out, indices.len() as u32);
    for &index in indices {
        push_u32(out, index);
    }
}

fn write_glyph(path: &Path, glyph: &Glyph) -> Result<()> {
    let mut out = Vec::new();
    push_u32(&mut out, glyph.revision);
    for value in [
        glyph.bounds.x_min,
        glyph.bounds.y_min,
        glyph.bounds.x_max,
        glyph.bounds.y_max,
    ] {
        push_f32(&mut out, value);
    }
    push_u32(&mut out, glyph.curves.len() as u32);
    for curve in &glyph.curves {
        for value in [
            curve.p0.x, curve.p0.y, curve.p1.x, curve.p1.y, curve.p2.x, curve.p2.y,
        ] {
            push_f32(&mut out, value);
        }
    }
    write_band_table(
        &mut out,
        &glyph.horizontal_bands,
        &glyph.horizontal_curve_indices,
    );
    write_band_table(
        &mut out,
        &glyph.vertical_bands,
        &glyph.vertical_curve_indices,
    );
    fs::write(path, out).with_context(|| format!("writing {}", path.display()))
}

fn polygon(points: &[Point], curves: &mut Vec<Curve>) {
    for pair in points.windows(2) {
        curves.push(Curve::line(pair[0], pair[1]));
    }
    curves.push(Curve::line(*points.last().unwrap(), points[0]));
}

fn rectangle(x0: f32, y0: f32, x1: f32, y1: f32, curves: &mut Vec<Curve>) {
    polygon(
        &[
            Point::new(x0, y0),
            Point::new(x0, y1),
            Point::new(x1, y1),
            Point::new(x1, y0),
        ],
        curves,
    );
}

fn circle(cx: f32, cy: f32, radius: f32, clockwise: bool, curves: &mut Vec<Curve>) {
    let top = Point::new(cx, cy + radius);
    let right = Point::new(cx + radius, cy);
    let bottom = Point::new(cx, cy - radius);
    let left = Point::new(cx - radius, cy);
    let mut add = |p0: Point, p1: Point, p2: Point| curves.push(Curve { p0, p1, p2 });
    if clockwise {
        add(top, Point::new(cx + radius, cy + radius), right);
        add(right, Point::new(cx + radius, cy - radius), bottom);
        add(bottom, Point::new(cx - radius, cy - radius), left);
        add(left, Point::new(cx - radius, cy + radius), top);
    } else {
        add(top, Point::new(cx - radius, cy + radius), left);
        add(left, Point::new(cx - radius, cy - radius), bottom);
        add(bottom, Point::new(cx + radius, cy - radius), right);
        add(right, Point::new(cx + radius, cy + radius), top);
    }
}

fn fixture_curves(name: &str) -> Result<Vec<Curve>> {
    let mut curves = Vec::new();
    match name {
        "H" => {
            rectangle(0.10, 0.10, 0.25, 0.90, &mut curves);
            rectangle(0.75, 0.10, 0.90, 0.90, &mut curves);
            rectangle(0.25, 0.43, 0.75, 0.57, &mut curves);
        }
        "A" => {
            polygon(
                &[
                    Point::new(0.06, 0.10),
                    Point::new(0.40, 0.90),
                    Point::new(0.54, 0.90),
                    Point::new(0.25, 0.10),
                ],
                &mut curves,
            );
            polygon(
                &[
                    Point::new(0.46, 0.90),
                    Point::new(0.60, 0.90),
                    Point::new(0.94, 0.10),
                    Point::new(0.75, 0.10),
                ],
                &mut curves,
            );
            rectangle(0.28, 0.36, 0.72, 0.49, &mut curves);
        }
        "V" => {
            polygon(
                &[
                    Point::new(0.06, 0.90),
                    Point::new(0.23, 0.90),
                    Point::new(0.52, 0.10),
                    Point::new(0.42, 0.10),
                ],
                &mut curves,
            );
            polygon(
                &[
                    Point::new(0.77, 0.90),
                    Point::new(0.94, 0.90),
                    Point::new(0.58, 0.10),
                    Point::new(0.48, 0.10),
                ],
                &mut curves,
            );
        }
        "M" => {
            rectangle(0.07, 0.10, 0.22, 0.90, &mut curves);
            rectangle(0.78, 0.10, 0.93, 0.90, &mut curves);
            polygon(
                &[
                    Point::new(0.19, 0.90),
                    Point::new(0.32, 0.90),
                    Point::new(0.55, 0.45),
                    Point::new(0.47, 0.30),
                ],
                &mut curves,
            );
            polygon(
                &[
                    Point::new(0.68, 0.90),
                    Point::new(0.81, 0.90),
                    Point::new(0.53, 0.30),
                    Point::new(0.45, 0.45),
                ],
                &mut curves,
            );
        }
        "g" => {
            circle(0.44, 0.59, 0.30, true, &mut curves);
            circle(0.44, 0.59, 0.16, false, &mut curves);
            polygon(
                &[
                    Point::new(0.65, 0.62),
                    Point::new(0.80, 0.62),
                    Point::new(0.80, 0.20),
                    Point::new(0.66, 0.20),
                ],
                &mut curves,
            );
            curves.push(Curve {
                p0: Point::new(0.80, 0.22),
                p1: Point::new(0.80, 0.02),
                p2: Point::new(0.58, 0.02),
            });
            curves.push(Curve::line(Point::new(0.58, 0.02), Point::new(0.38, 0.02)));
            curves.push(Curve::line(Point::new(0.38, 0.02), Point::new(0.38, 0.15)));
            curves.push(Curve::line(Point::new(0.38, 0.15), Point::new(0.58, 0.15)));
            curves.push(Curve {
                p0: Point::new(0.58, 0.15),
                p1: Point::new(0.66, 0.15),
                p2: Point::new(0.66, 0.22),
            });
            curves.push(Curve::line(Point::new(0.66, 0.22), Point::new(0.80, 0.22)));
        }
        "o" => {
            circle(0.50, 0.50, 0.39, true, &mut curves);
            circle(0.50, 0.50, 0.23, false, &mut curves);
        }
        "plus" => {
            rectangle(0.12, 0.43, 0.88, 0.57, &mut curves);
            rectangle(0.43, 0.12, 0.57, 0.88, &mut curves);
        }
        "pipe" => rectangle(0.43, 0.08, 0.57, 0.92, &mut curves),
        _ => bail!("no fixture definition for {name}"),
    }
    Ok(curves)
}

fn make_bands(curves: &[Curve], lower: f32, upper: f32, horizontal: bool) -> (Vec<Band>, Vec<u32>) {
    let width = (upper - lower) / BAND_COUNT as f32;
    let mut bands = Vec::with_capacity(BAND_COUNT);
    let mut indices = Vec::new();
    for band_index in 0..BAND_COUNT {
        let band_lower = lower + band_index as f32 * width;
        let band_upper = if band_index + 1 == BAND_COUNT {
            upper
        } else {
            band_lower + width
        };
        let first_curve = indices.len() as u32;
        let mut members: Vec<(usize, f32)> = curves
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, curve)| {
                let (axis_min, axis_max) = if horizontal {
                    curve.y_extent()
                } else {
                    curve.x_extent()
                };
                if axis_max - axis_min < BAND_EPSILON_EM
                    || axis_max < band_lower - BAND_EPSILON_EM
                    || axis_min > band_upper + BAND_EPSILON_EM
                {
                    return None;
                }
                let sort_key = if horizontal {
                    curve.x_extent().1
                } else {
                    curve.y_extent().1
                };
                Some((index, sort_key))
            })
            .collect();
        members.sort_by(|(index_a, key_a), (index_b, key_b)| {
            key_b.total_cmp(key_a).then_with(|| index_a.cmp(index_b))
        });
        indices.extend(members.iter().map(|(index, _)| *index as u32));
        bands.push(Band {
            lower: band_lower,
            upper: band_upper,
            first_curve,
            curve_count: members.len() as u32,
        });
    }
    (bands, indices)
}

fn build_fixture(name: &str) -> Result<Glyph> {
    let curves = fixture_curves(name)?;
    let bounds = curves.iter().copied().fold(
        Bounds {
            x_min: f32::INFINITY,
            y_min: f32::INFINITY,
            x_max: f32::NEG_INFINITY,
            y_max: f32::NEG_INFINITY,
        },
        |mut bounds, curve| {
            let (x_min, x_max) = curve.x_extent();
            let (y_min, y_max) = curve.y_extent();
            bounds.x_min = bounds.x_min.min(x_min);
            bounds.y_min = bounds.y_min.min(y_min);
            bounds.x_max = bounds.x_max.max(x_max);
            bounds.y_max = bounds.y_max.max(y_max);
            bounds
        },
    );
    let (horizontal_bands, horizontal_curve_indices) =
        make_bands(&curves, bounds.y_min, bounds.y_max, true);
    let (vertical_bands, vertical_curve_indices) =
        make_bands(&curves, bounds.x_min, bounds.x_max, false);
    Ok(Glyph {
        revision: GLYPH_REVISION,
        bounds,
        curves,
        horizontal_bands,
        horizontal_curve_indices,
        vertical_bands,
        vertical_curve_indices,
    })
}

fn write_fixtures(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for name in GLYPH_NAMES {
        write_glyph(&dir.join(format!("{name}.slug")), &build_fixture(name)?)?;
    }
    println!(
        "wrote {} SlugGlyph fixtures to {}",
        GLYPH_NAMES.len(),
        dir.display()
    );
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 4],
    tex: [f32; 4],
    jac: [f32; 4],
    bnd: [f32; 4],
    col: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    matrix: [[f32; 4]; 4],
    viewport: [f32; 4],
}

#[derive(Debug)]
struct PackedTextures {
    curve_texels: Vec<[f32; 4]>,
    curve_height: u32,
    band_texels: Vec<[u32; 4]>,
    band_height: u32,
}

#[derive(Clone, Copy, Debug)]
struct RenderDimensions {
    width: u32,
    height: u32,
    em_pixels: f32,
    padding: f32,
}

fn linear_to_xy(index: u32, width: u32) -> [u32; 2] {
    [index % width, index / width]
}

fn pack_textures(glyph: &Glyph) -> Result<PackedTextures> {
    ensure!(
        glyph.revision == GLYPH_REVISION,
        "unsupported fixture revision {}; expected {}",
        glyph.revision,
        GLYPH_REVISION
    );
    let curve_texel_count = u32::try_from(glyph.curves.len())? * 2;
    let curve_height = curve_texel_count.div_ceil(CURVE_TEXTURE_WIDTH).max(1);
    let mut curve_texels = vec![[0.0; 4]; usize::try_from(CURVE_TEXTURE_WIDTH * curve_height)?];
    for (index, curve) in glyph.curves.iter().enumerate() {
        let first = index * 2;
        curve_texels[first] = [curve.p0.x, curve.p0.y, curve.p1.x, curve.p1.y];
        curve_texels[first + 1] = [curve.p2.x, curve.p2.y, 0.0, 0.0];
    }

    let header_count = glyph.horizontal_bands.len() + glyph.vertical_bands.len();
    let list_count = glyph.horizontal_curve_indices.len() + glyph.vertical_curve_indices.len();
    let band_texel_count = u32::try_from(header_count + list_count)?;
    let band_height = band_texel_count.div_ceil(BAND_TEXTURE_WIDTH).max(1);
    let mut band_texels = vec![[0; 4]; usize::try_from(BAND_TEXTURE_WIDTH * band_height)?];
    let mut list_cursor = header_count;
    let mut write_table = |bands: &[Band], curve_indices: &[u32], header_base: usize| {
        for (band_index, band) in bands.iter().enumerate() {
            let start = usize::try_from(band.first_curve).unwrap();
            let end = start + usize::try_from(band.curve_count).unwrap();
            let members = &curve_indices[start..end];
            band_texels[header_base + band_index] =
                [members.len() as u32, list_cursor as u32, 0, 0];
            for &curve_index in members {
                let location = linear_to_xy(curve_index * 2, CURVE_TEXTURE_WIDTH);
                band_texels[list_cursor] = [location[0], location[1], 0, 0];
                list_cursor += 1;
            }
        }
    };
    write_table(&glyph.horizontal_bands, &glyph.horizontal_curve_indices, 0);
    write_table(
        &glyph.vertical_bands,
        &glyph.vertical_curve_indices,
        glyph.horizontal_bands.len(),
    );
    ensure!(
        list_cursor == header_count + list_count,
        "internal band packing mismatch"
    );
    Ok(PackedTextures {
        curve_texels,
        curve_height,
        band_texels,
        band_height,
    })
}

fn selected_backends() -> wgpu::Backends {
    match std::env::var("WGPU_BACKEND").ok().as_deref() {
        Some("vulkan") => wgpu::Backends::VULKAN,
        Some("metal") => wgpu::Backends::METAL,
        Some("dx12") => wgpu::Backends::DX12,
        Some("gl") => wgpu::Backends::GL,
        Some(other) => {
            eprintln!("warning: unknown WGPU_BACKEND={other:?}; enabling all backends");
            wgpu::Backends::all()
        }
        None => wgpu::Backends::all(),
    }
}

fn render(args: &Args) -> Result<()> {
    let glyph_path = PathBuf::from("glyphs").join(format!("{}.slug", args.glyph));
    let glyph = read_glyph(&glyph_path)?;
    let packed = pack_textures(&glyph)?;
    let shader_source = fs::read_to_string(&args.shader)
        .with_context(|| format!("reading {}", args.shader.display()))?;

    let width = (BASE_CANVAS_PIXELS * args.dpi).round() as u32;
    let height = width;
    let em_pixels = BASE_EM_PIXELS * args.dpi;
    let padding = BASE_PADDING_PIXELS * args.dpi;
    ensure!(width > 0 && height > 0, "DPI produced an empty target");

    pollster::block_on(render_async(
        args,
        &glyph,
        &packed,
        &shader_source,
        RenderDimensions {
            width,
            height,
            em_pixels,
            padding,
        },
    ))
}

fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder
        .read_info()
        .with_context(|| format!("reading PNG header from {}", path.display()))?;
    ensure!(
        reader.info().color_type == png::ColorType::Rgba
            && reader.info().bit_depth == png::BitDepth::Eight,
        "{} must be an 8-bit RGBA PNG",
        path.display()
    );
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .with_context(|| format!("decoding {}", path.display()))?;
    buffer.truncate(info.buffer_size());
    Ok((info.width, info.height, buffer))
}

fn compare_pngs(known_good: &Path, mutant: &Path, threshold: u8) -> Result<()> {
    let (width, height, known_pixels) = read_png(known_good)?;
    let (mutant_width, mutant_height, mutant_pixels) = read_png(mutant)?;
    ensure!(
        (width, height) == (mutant_width, mutant_height),
        "PNG dimensions differ: known-good={}x{}, mutant={}x{}",
        width,
        height,
        mutant_width,
        mutant_height
    );
    let mut differing_pixels = 0u64;
    let mut differing_columns = 0u32;
    let mut max_column = 0u32;
    let mut max_column_differing_pixels = 0u32;
    let mut max_channel_delta = 0u8;
    for x in 0..width {
        let mut column_differing_pixels = 0u32;
        for y in 0..height {
            let offset = usize::try_from((y * width + x) * 4)?;
            let pixel_delta = known_pixels[offset..offset + 4]
                .iter()
                .zip(&mutant_pixels[offset..offset + 4])
                .map(|(known, changed)| known.abs_diff(*changed))
                .max()
                .unwrap();
            max_channel_delta = max_channel_delta.max(pixel_delta);
            if pixel_delta > threshold {
                differing_pixels += 1;
                column_differing_pixels += 1;
            }
        }
        if column_differing_pixels > 0 {
            differing_columns += 1;
        }
        if column_differing_pixels > max_column_differing_pixels {
            max_column = x;
            max_column_differing_pixels = column_differing_pixels;
        }
    }
    ensure!(
        differing_pixels > 0,
        "sensitivity failure: no pixel differs above threshold {threshold}"
    );
    println!(
        "PASS threshold={} differing_pixels={} differing_columns={}/{} \
         max_column={} max_column_differing_pixels={}/{} max_channel_delta={}",
        threshold,
        differing_pixels,
        differing_columns,
        width,
        max_column,
        max_column_differing_pixels,
        height,
        max_channel_delta
    );
    Ok(())
}

async fn render_async(
    args: &Args,
    glyph: &Glyph,
    packed: &PackedTextures,
    shader_source: &str,
    dimensions: RenderDimensions,
) -> Result<()> {
    let RenderDimensions {
        width,
        height,
        em_pixels,
        padding,
    } = dimensions;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: selected_backends(),
        ..Default::default()
    });
    let fallback_adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: true,
            compatible_surface: None,
        })
        .await;
    let adapter = match fallback_adapter {
        Ok(adapter) => adapter,
        Err(_) => instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .context("requesting a wgpu adapter")?,
    };
    let info = adapter.get_info();
    eprintln!(
        "adapter={} backend={:?} device_type={:?} driver={} {}",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    );
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Slug reference device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: 4096,
                ..wgpu::Limits::downlevel_defaults()
            },
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await
        .context("requesting a wgpu device")?;

    let sx = 2.0 * em_pixels / width as f32;
    let sy = 2.0 * em_pixels / height as f32;
    let tx = -1.0 + 2.0 * padding / width as f32;
    let ty = -1.0 + 2.0 * padding / height as f32;
    let params = Params {
        matrix: [
            [sx, 0.0, 0.0, tx],
            [0.0, sy, 0.0, ty],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        viewport: [width as f32, height as f32, 0.0, 0.0],
    };
    let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Slug parameters"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let curve_texture = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("Slug curve texture"),
            size: wgpu::Extent3d {
                width: CURVE_TEXTURE_WIDTH,
                height: packed.curve_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&packed.curve_texels),
    );
    let band_texture = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("Slug band texture"),
            size: wgpu::Extent3d {
                width: BAND_TEXTURE_WIDTH,
                height: packed.band_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&packed.band_texels),
    );
    let curve_view = curve_texture.create_view(&Default::default());
    let band_view = band_texture.create_view(&Default::default());

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Slug reference bindings"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Slug reference bind group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&curve_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&band_view),
            },
        ],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Pinned Slug HLSL to WGSL reference port"),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Slug reference pipeline layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let vertex_attributes = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Slug reference pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attributes,
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });

    let vertical_count = glyph.vertical_bands.len() as f32;
    let horizontal_count = glyph.horizontal_bands.len() as f32;
    let bnd = [
        vertical_count / (glyph.bounds.x_max - glyph.bounds.x_min),
        horizontal_count / (glyph.bounds.y_max - glyph.bounds.y_min),
        -glyph.bounds.x_min * vertical_count / (glyph.bounds.x_max - glyph.bounds.x_min),
        -glyph.bounds.y_min * horizontal_count / (glyph.bounds.y_max - glyph.bounds.y_min),
    ];
    let glyph_location_bits = 0u32;
    let glyph_max_bits =
        ((glyph.horizontal_bands.len() as u32 - 1) << 16) | (glyph.vertical_bands.len() as u32 - 1);
    let tex_bits = [
        f32::from_bits(glyph_location_bits),
        f32::from_bits(glyph_max_bits),
    ];
    let corners = [
        ([glyph.bounds.x_min, glyph.bounds.y_min], [-1.0, -1.0]),
        ([glyph.bounds.x_max, glyph.bounds.y_min], [1.0, -1.0]),
        ([glyph.bounds.x_max, glyph.bounds.y_max], [1.0, 1.0]),
        ([glyph.bounds.x_min, glyph.bounds.y_max], [-1.0, 1.0]),
    ];
    let make_vertex = |corner: usize| {
        let (position, normal) = corners[corner];
        Vertex {
            pos: [position[0], position[1], normal[0], normal[1]],
            tex: [position[0], position[1], tex_bits[0], tex_bits[1]],
            jac: [1.0, 0.0, 0.0, 1.0],
            bnd,
            col: [1.0, 1.0, 1.0, 1.0],
        }
    };
    let vertices = [
        make_vertex(0),
        make_vertex(1),
        make_vertex(2),
        make_vertex(0),
        make_vertex(2),
        make_vertex(3),
    ];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Slug glyph quad"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let output_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Slug PNG target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output_texture.create_view(&Default::default());
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Slug PNG readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Slug reference commands"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Slug reference pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.draw(0..6, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device
        .poll(wgpu::PollType::Wait)
        .context("waiting for Slug readback")?;
    receiver
        .recv()
        .context("receiving Slug readback notification")?
        .context("mapping Slug readback")?;
    let mapped = readback.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity(usize::try_from(unpadded_bytes_per_row * height)?);
    for row in mapped.chunks_exact(usize::try_from(padded_bytes_per_row)?) {
        pixels.extend_from_slice(&row[..usize::try_from(unpadded_bytes_per_row)?]);
    }
    drop(mapped);
    readback.unmap();

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let file = fs::File::create(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    encoder.set_filter(png::FilterType::Sub);
    let mut writer = encoder.write_header().context("writing PNG header")?;
    writer
        .write_image_data(&pixels)
        .context("writing PNG data")?;
    writer.finish().context("finishing PNG")?;
    println!(
        "rendered glyph={} dpi={} size={}x{} shader={} output={}",
        args.glyph,
        args.dpi,
        width,
        height,
        args.shader.display(),
        args.output.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = parse_args()?;
    if let Some(dir) = &args.write_fixtures {
        return write_fixtures(dir);
    }
    if let Some((known_good, mutant, threshold)) = &args.compare {
        return compare_pngs(known_good, mutant, *threshold);
    }
    render(&args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_encoding_round_trips() {
        let temp = std::env::temp_dir().join(format!(
            "mkui-slug-reference-harness-{}",
            std::process::id()
        ));
        fs::create_dir_all(&temp).unwrap();
        for name in GLYPH_NAMES {
            let path = temp.join(format!("{name}.slug"));
            let expected = build_fixture(name).unwrap();
            write_glyph(&path, &expected).unwrap();
            let actual = read_glyph(&path).unwrap();
            assert_eq!(actual.revision, GLYPH_REVISION);
            assert_eq!(actual.curves.len(), expected.curves.len());
            assert_eq!(
                actual.horizontal_curve_indices,
                expected.horizontal_curve_indices
            );
            assert_eq!(
                actual.vertical_curve_indices,
                expected.vertical_curve_indices
            );
        }
    }
}

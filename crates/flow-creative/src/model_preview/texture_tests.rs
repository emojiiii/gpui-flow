use super::tests::{Fixture, frontal_camera, pixel};
use super::*;
use image::{ImageEncoder, Rgba, RgbaImage};
use serde_json::json;

fn add_texture(fixture: &mut Fixture, uvs: &[[f32; 2]], image: RgbaImage) {
    while !fixture.bin.len().is_multiple_of(4) {
        fixture.bin.push(0);
    }
    let offset = fixture.bin.len();
    for uv in uvs {
        for value in uv {
            fixture.bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let views = fixture.json["bufferViews"].as_array_mut().unwrap();
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":uvs.len()*8}));
    let accessors = fixture.json["accessors"].as_array_mut().unwrap();
    let accessor = accessors.len();
    accessors.push(json!({"bufferView":view,"componentType":5126,"count":uvs.len(),"type":"VEC2"}));
    fixture.json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = json!(accessor);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let offset = fixture.bin.len();
    fixture.bin.extend_from_slice(&png);
    let views = fixture.json["bufferViews"].as_array_mut().unwrap();
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":png.len()}));
    fixture.json["images"] = json!([{"bufferView":view,"mimeType":"image/png"}]);
    fixture.json["textures"] = json!([{"source":0,"sampler":0}]);
    fixture.json["samplers"] =
        json!([{"magFilter":9728,"minFilter":9728,"wrapS":33071,"wrapT":33071}]);
    fixture.json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index":0});
    fixture.json["buffers"][0]["byteLength"] = json!(fixture.bin.len());
}
fn quad(sloped: bool) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.add(
        &[
            [-1., -1., 0.],
            [1., -1., 0.],
            [1., 1., if sloped { -2. } else { 0. }],
            [-1., 1., if sloped { -2. } else { 0. }],
        ],
        Some(&[0, 1, 2, 0, 2, 3]),
        12,
        2,
        [1.; 4],
    );
    fixture
}
const UV: [[f32; 2]; 4] = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];

#[test]
fn embedded_base_color_keeps_uv_origin_and_opaque_alpha() {
    let mut fixture = quad(false);
    let colors = [
        [255, 0, 0, 0],
        [0, 255, 0, 64],
        [0, 0, 255, 128],
        [255, 255, 0, 255],
    ];
    add_texture(
        &mut fixture,
        &UV,
        RgbaImage::from_fn(2, 2, |x, y| Rgba(colors[(y * 2 + x) as usize])),
    );
    let source = fixture.bytes();
    let mesh = parse_glb(&source).unwrap();
    assert_eq!(mesh.texture_count(), 1);
    assert_eq!(mesh.texture_bytes(), 16);
    let frame = render(&mesh, &frontal_camera(), 100, 100).unwrap();
    let tl = pixel(&frame, 30, 30);
    let tr = pixel(&frame, 70, 30);
    let bl = pixel(&frame, 30, 70);
    let br = pixel(&frame, 70, 70);
    assert!(tl[0] > 200 && tl[1] == 0 && tl[2] == 0);
    assert!(tr[1] > 200 && tr[0] == 0 && tr[2] == 0);
    assert!(bl[2] > 200 && bl[0] == 0 && bl[1] == 0);
    assert!(br[0] > 200 && br[1] > 200 && br[2] == 0);
    assert!([tl, tr, bl, br].iter().all(|p| p[3] == 255));
    assert_eq!(source, fixture.bytes());
    assert!(
        render_cancellable(&mesh, &frontal_camera(), 100, 100, &AtomicBool::new(true)).is_err()
    );
}

#[test]
fn sloped_textures_match_analytic_ray_plane_uv_and_triangle_order() {
    let mut fixture = quad(true);
    add_texture(
        &mut fixture,
        &UV,
        RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 80, 255])),
    );
    let mesh = fixture.parse().unwrap();
    let camera = frontal_camera();
    let frame = render(&mesh, &camera, 100, 100).unwrap();
    let focal = 100. / (2. * (PI / 8.).tan());
    let normal = Vec3(0., -1., 1.).normalized();
    let light = Vec3(-0.35, 0.65, -1.).normalized();
    let shade = 0.25 + 0.75 * normal.dot(light).abs();
    for (x, y) in [(40, 35), (57, 41), (35, 56), (64, 61)] {
        let dx = (x as f64 + 0.5 - 50.) / focal;
        let dy = (50. - y as f64 - 0.5) / focal;
        // Camera ray from (0,0,3) intersects the authored plane z=-y-1.
        let t = 4. / (1. - dy);
        let uv = [(t * dx + 1.) / 2., (1. - t * dy) / 2.];
        let sampled = mesh.textures.get(0).unwrap().sample(uv, false);
        let expected = [
            linear_to_srgb(sampled[0] * shade),
            linear_to_srgb(sampled[1] * shade),
            linear_to_srgb(sampled[2] * shade),
            255,
        ];
        assert_eq!(pixel(&frame, x, y), expected, "ray-plane UV at {x},{y}");
    }
    let mut reversed = mesh.clone();
    reversed.triangles.reverse();
    assert_eq!(frame, render(&reversed, &camera, 100, 100).unwrap());
}

#[test]
fn textured_uv_contract_rejects_missing_unmatched_and_nonfinite_data() {
    let mut fixture = quad(false);
    add_texture(
        &mut fixture,
        &UV,
        RgbaImage::from_pixel(1, 1, Rgba([255; 4])),
    );
    let mut missing = fixture.json.clone();
    missing["meshes"][0]["primitives"][0]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("TEXCOORD_0");
    let original = std::mem::replace(&mut fixture.json, missing);
    assert!(fixture.parse().is_err());
    fixture.json = original;
    fixture.json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"] = json!(1);
    assert!(fixture.parse().is_err());
    fixture.json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"] = json!(0);
    let accessor = fixture.json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"]
        .as_u64()
        .unwrap() as usize;
    let view = fixture.json["accessors"][accessor]["bufferView"]
        .as_u64()
        .unwrap() as usize;
    let offset = fixture.json["bufferViews"][view]["byteOffset"]
        .as_u64()
        .unwrap() as usize;
    fixture.bin[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(fixture.parse().is_err());
}

#[test]
fn near_clip_intersections_preserve_texture_coordinates() {
    let polygon = clip_polygon(
        &[
            ClipVertex {
                point: Vec3(-1., 0., 1.),
                uv: [0., 0.],
            },
            ClipVertex {
                point: Vec3(1., 0., 1.),
                uv: [1., 0.],
            },
            ClipVertex {
                point: Vec3(0., 1., -1.),
                uv: [0.5, 1.],
            },
        ],
        Vec3(0., 0., 1.),
        0.,
    );
    let mut intersections = polygon
        .iter()
        .filter(|v| v.point.2.abs() < 1e-12)
        .map(|v| v.uv)
        .collect::<Vec<_>>();
    intersections.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(intersections, vec![[0.25, 0.5], [0.75, 0.5]]);
}

#[test]
fn legacy_large_textures_do_not_block_project_open_and_new_imports_stay_atomic() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Home(std::path::PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let suffix = format!(
        "flow-stored-texture-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = Home(std::env::temp_dir().join(suffix));
    fs::create_dir(&home.0).unwrap();
    let mut fixture = quad(false);
    add_texture(
        &mut fixture,
        &UV,
        RgbaImage::from_pixel(2048, 1, Rgba([180, 90, 25, 255])),
    );
    let bytes = fixture.bytes();
    assert!(matches!(
        parse_glb(&bytes),
        Err(PreviewError::BudgetExceeded(_))
    ));
    validate_stored_glb(&bytes).unwrap();
    let registry = crate::standard_registry();
    let store = crate::ProjectStore::new(home.0.join("projects")).unwrap();
    let mut project = store
        .create_project(
            "Legacy model",
            &gpui_flow_core::FlowDocument::default(),
            &registry,
        )
        .unwrap();
    let source = home.0.join("oversized-texture.glb");
    fs::write(&source, &bytes).unwrap();
    let project_path = project.directory().join("project.json");
    let before = fs::read(&project_path).unwrap();
    assert!(project.import_model(&source).is_err());
    assert_eq!(fs::read(&project_path).unwrap(), before);
    assert_eq!(
        fs::read_dir(project.directory().join("assets"))
            .unwrap()
            .count(),
        0
    );
    // Reproduce a file legitimately admitted before texture decoding existed.
    let key = "assets/legacy.glb";
    fs::write(project.directory().join(key), &bytes).unwrap();
    let mut document = project.document().clone();
    document
        .graph
        .add_node(
            registry
                .make_node(
                    "model",
                    "old-model",
                    gpui_flow_core::Point::new(120., 240.),
                    Some(json!({"asset":key,"caption":"kept"})),
                )
                .unwrap(),
        )
        .unwrap();
    let mut envelope: serde_json::Value = serde_json::from_slice(&before).unwrap();
    envelope["assets"] = json!([{"key":key,"name":"legacy.glb","media_type":"model/gltf-binary","byte_len":bytes.len(),"width":0,"height":0}]);
    envelope["document"] = serde_json::to_value(&document).unwrap();
    let saved = serde_json::to_vec(&envelope).unwrap();
    fs::write(&project_path, &saved).unwrap();
    let opened = store
        .open_project(&project.metadata().id, &registry)
        .unwrap();
    assert_eq!(opened.document(), &document);
    assert_eq!(fs::read(opened.resolve_asset(key).unwrap()).unwrap(), bytes);
    assert_eq!(fs::read(&project_path).unwrap(), saved);
    fixture.json["images"][0] = json!({"uri":"https://example.invalid/texture.png"});
    assert!(
        validate_stored_glb(&fixture.bytes()).is_err(),
        "legacy admission still rejects every URI"
    );
}

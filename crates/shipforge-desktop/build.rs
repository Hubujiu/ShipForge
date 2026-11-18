use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/brand.svg");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Generate every Windows shell/DPI size from the same embedded brand artwork.
    // Keep the ICO in OUT_DIR so it cannot drift from the workbench's SVG.
    let tree = resvg::usvg::Tree::from_data(
        &fs::read("assets/brand.svg").expect("read brand SVG"),
        &resvg::usvg::Options::default(),
    )
    .expect("parse brand SVG");
    let sizes = [16_u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let images: Vec<_> = sizes
        .iter()
        .map(|&size| {
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("icon pixmap");
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(
                    size as f32 / tree.size().width(),
                    size as f32 / tree.size().height(),
                ),
                &mut pixmap.as_mut(),
            );
            pixmap.encode_png().expect("encode icon PNG")
        })
        .collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + sizes.len() as u32 * 16;
    for (&size, image) in sizes.iter().zip(&images) {
        ico.extend_from_slice(&[size as u8, size as u8, 0, 0]);
        ico.extend_from_slice(&1_u16.to_le_bytes());
        ico.extend_from_slice(&32_u16.to_le_bytes());
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        ico.extend_from_slice(&image);
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let icon = output.join("shipforge.ico");
    fs::write(&icon, ico).expect("write Windows icon");
    let resource = output.join("shipforge.rc");
    // GPUI's Windows platform loads resource 1 for the window/taskbar icon.
    fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write Windows resource");
    embed_resource::compile(resource, embed_resource::NONE)
        .manifest_required()
        .expect("embed Windows application icon");
}

use std::env;

use anyhow::Result;
use bvh_anim_parser::parse::load_bvh_from_file;
use bvh_to_gav::bvh_to_gav;
use ndarray_npy::write_npy;

fn convert_bvh_to_gav(source_folder: &str) -> Result<usize> {
    let mut count = 0;
    for file in std::fs::read_dir(source_folder).unwrap() {
        let file = file.unwrap();
        let path = file.path();
        let output_path = path.with_extension("npy");
        if path.extension().map(|s| s == "bvh").unwrap_or(false) {
            // Call the conversion function here
            if let Some(path) = file.path().to_str() {
                let (bvh_meta, bvh_data) = load_bvh_from_file(path);
                let gav_tensor = bvh_to_gav(&bvh_data, bvh_meta.num_frames)?;
                write_npy(output_path, &gav_tensor)?;
                count += 1;
            }
        }
    }

    Ok(count)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <source_folder>", args[0]);
        std::process::exit(1);
    }
    let source_folder = &args[1];
    match convert_bvh_to_gav(source_folder) {
        Ok(0) => println!("No BVH files found to convert"),
        Ok(count) => println!("Successfully converted {} BVH files to GAV", count),
        Err(e) => eprintln!("Error converting BVH to GAV: {}", e),
    }
}

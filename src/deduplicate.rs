use crate::errors::AppError;
use crate::models::{DeduplicationReport, DuplicatesGroup, ImageInfo};
use image_hasher::Hasher;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json;
use std::collections::HashSet;
use std::fs::{self};
use std::path::{Path, PathBuf};
use rayon::prelude::*;

fn get_image_hashes(directory: &Path, hasher: &Hasher) -> Result<Vec<ImageInfo>, AppError> {
    if !directory.is_dir() {
        return Err(AppError::InvalidDirectory(directory.to_path_buf()));
    }

    let entries: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(|x| Result::ok(x))
        .map(|x| x.path())
        .collect();

    let bar = ProgressBar::new(entries.len() as u64);
    bar.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} files")
            .unwrap(),
    );

    let image_hashes: Vec<ImageInfo> = entries
        .par_iter()
        .filter_map(|path| {
            if let Ok(img) = image::open(path) {
                let hash = hasher.hash_image(&img);
                bar.inc(1);
                Some(ImageInfo {
                    path: path.clone(),
                    hash: hash,
                })
            } else {
                None
            }
        })
        .collect();

    Ok(image_hashes)
}

fn find_duplicates(images: Vec<ImageInfo>, duplicate_threshold: u32) -> Vec<DuplicatesGroup> {
    let mut groups: Vec<DuplicatesGroup> = Vec::new();
    let mut processed: HashSet<PathBuf> = HashSet::new();

    for (i, image) in images.iter().enumerate() {
        if processed.contains(&image.path) {
            continue;
        }

        let mut current_group: Vec<ImageInfo> = vec![image.clone()];

        for other_image in images.iter().skip(i + 1) {
            if processed.contains(&other_image.path) {
                continue;
            }

            if image.hash.dist(&other_image.hash) < duplicate_threshold {
                current_group.push(other_image.clone());
                processed.insert(other_image.path.clone());
            }
        }

        if current_group.len() > 1 {
            groups.push(DuplicatesGroup {
                items: current_group,
            });
            processed.insert(image.path.clone());
        }
    }

    groups
}

fn save_results(report: &DeduplicationReport, path: &Path) -> Result<(), AppError> {
    let contents = serde_json::to_string_pretty(report)?;
    fs::write(path, contents)?;
    println!("Deduplication report saved to {:?}", path);
    Ok(())
}

pub fn run(
    directory: String,
    duplicate_threshold: u32,
    hash_size: u32, 
    report_filename: &str,
) -> Result<(), AppError> {
    let dir = Path::new(&directory);

    let hasher = image_hasher::HasherConfig::new().hash_size(hash_size, hash_size).to_hasher();
    println!("Starting deduplication in directory: {:?}", dir);

    let image_hashes = get_image_hashes(dir, &hasher)?;
    println!("Found {} images.", image_hashes.len());

    let duplicates = find_duplicates(image_hashes, duplicate_threshold);
    println!("Found {} duplicate groups.", duplicates.len());

    let output_path = dir.join(report_filename);
    let report = DeduplicationReport::new(dir.to_path_buf(), duplicates, duplicate_threshold);

    println!("Saving deduplication report...");
    save_results(&report, &output_path)?;

    println!("Process completed successfully.\n");
    println!("{}", report);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;
    use image_hasher::HasherConfig;
    use image_hasher::ImageHash;
    use tempfile::tempdir;

    #[test]
    fn test_get_image_hashes() {
        let dir = tempdir().unwrap();
        let image_path = dir.path().join("image.png");

        let mut image: RgbImage = RgbImage::new(100, 100);
        *image.get_pixel_mut(5, 5) = image::Rgb([255, 255, 255]);
        image.save(&image_path).unwrap();

        let hasher = HasherConfig::new()
            .hash_size(16, 16)
            .to_hasher();
        let result = get_image_hashes(dir.path(), &hasher);

        assert!(result.is_ok());
        let image_hashes = result.unwrap();
        assert_eq!(image_hashes.len(), 1);
        assert_eq!(image_hashes[0].path, image_path);
    }

    #[test]
    fn test_find_duplicates() {
        let hash1: ImageHash = ImageHash::from_base64("DAIDBwMHAf8").unwrap();
        let hash2: ImageHash = ImageHash::from_base64("8/JwVtbOVy4").unwrap();
        let hash3: ImageHash = hash1.clone();
        let hash4: ImageHash = ImageHash::from_base64("DwcHBwcHBwc").unwrap(); 
        let hash5: ImageHash = ImageHash::from_base64("HxcXB4cGBgc").unwrap();

        let image1 = ImageInfo {
            path: PathBuf::from("image1.png"),
            hash: hash1,
        };
        let image2 = ImageInfo {
            path: PathBuf::from("image2.png"),
            hash: hash2,
        };
        let image3 = ImageInfo {
            path: PathBuf::from("image3.png"),
            hash: hash3, // Duplicate of image1
        };
        let image4 = ImageInfo {
            path: PathBuf::from("image4.png"),
            hash: hash4,
        };
        let image5 = ImageInfo {
            path: PathBuf::from("image5.png"),
            hash: hash5,
        };

        let images = vec![image1.clone(), image2.clone(), image3.clone(), image4.clone(), image5.clone()];
        let groups = find_duplicates(images, 10u32);

        assert_eq!(groups.len(), 2, "Expected two groups of duplicates");
        assert_eq!(
            groups[0].items.len(),
            2,
            "Expected two items in the duplicate group"
        );
        assert!(groups[0].items.contains(&image1));
        assert!(groups[0].items.contains(&image3));
    }
}

use std::cmp::min;
use std::fs;

use std::io::Cursor;
use std::io::Write;

use std::path::Path;

use std::process::Command;

use std::slice;

use chrono::NaiveDateTime;

use serde::{ Deserialize, Serialize };

use image::codecs::jpeg::JpegEncoder;
use image::io::Reader as ImageReader;
use image::ImageError;
use image::DynamicImage;
use image::GenericImage;
use image::imageops::FilterType;
use image::imageops::resize;
use image::imageops::thumbnail;
use image::ImageBuffer;
use image::ImageResult;
use image::Rgba;

pub static MD_FILE : &str = "metadata";

#[derive(Serialize, Deserialize, Debug)]
pub struct Image {
    pub path: String,
    pub caption: String,
    pub time: NaiveDateTime,
    pub width: u16,
    pub height: u16,
    pub mp4_scaled: bool,
    pub location: Option<String>
}

impl Image {
    pub fn is_mp4(&self) -> bool {
        is_mp4(&self.path)
    }
}

pub fn load_metadata(path: &Path) -> std::io::Result<Vec<Image>> {
    let mut buffer = Vec::<u8>::new();
    load_file(path, &mut buffer);
    let images = match serde_json::from_reader(Cursor::new(unsafe {
        slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len())})) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Error readng metadata, which is\n{}", std::str::from_utf8(&buffer).unwrap());
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        };
    
    Ok(images)
}

pub fn load_file(path: &Path, buffer: &mut dyn Write) {
    match std::fs::read(path) {
        Ok(bytes) => {
            let _ = buffer.write(bytes.as_slice());
        }
        Err(e) => {
            eprintln!("File {} can't be read {:?}", path.display(), e);
        }
    }
}

pub fn update_caption(path: &Path, id: usize, caption: &String) -> std::io::Result<()> {
    let mut images = load_metadata(&path).expect(format!("Metadata doesn't exist at {}", path.display()).as_str());
    match images.get_mut(id) {
        Some(i) => {
            i.caption = caption.clone();
        },
        None => {
            eprintln!("Can't write caption at {} to {}", id, path.display());
        }
    }
    match fs::write(path.as_os_str(), serde_json::to_string_pretty(&images).unwrap()) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Unable to write {} with error {}", path.display(), e.to_string());
            return Err(e);
        }
    }
}

pub fn read_image(path: &Path) -> ImageResult<DynamicImage> {
    let f = match ImageReader::open(&path)?.with_guessed_format() {
        Ok(v) => v,
        Err(e) => return Err(ImageError::IoError(e))
    };
    f.decode() 
}

pub const THUMBNAIL_SIZE: u32 = 100;

pub fn as_scaled(file_name: &String) -> String {
    return format!("{}.scaled.mp4", file_name);
}

pub fn as_preview(file_name: &String) -> String {
    return format!("{}.preview.jpg", file_name);
}

pub fn is_jpg(file_name: &String) -> bool {
    return file_name.ends_with(".jpg") || file_name.ends_with(".JPG");
}

pub fn is_mp4(file_name: &String) -> bool {
    return file_name.ends_with(".mp4") || file_name.ends_with(".MP4") ||
           file_name.ends_with(".mov") || file_name.ends_with(".MOV") ||
           file_name.ends_with(".avi") || file_name.ends_with(".AVI");
}

static TMP_FILE: &str = "/tmp/out.jpg";

/**
  Makes the thumbnails and previews for MP4s
*/
pub fn make_preview(images: &Vec<Image>) {
    const MAX_ROWS_IN_JPG: usize = 0x10000;  // jpgs can have at most 64K rows
    const MAX_TN_COUNT: usize = MAX_ROWS_IN_JPG / THUMBNAIL_SIZE as usize;
    
    let mut start: usize = 0;
    let mut end: usize = min(images.len(), MAX_TN_COUNT);

    let mut img_no = 0;
    loop {
        let postfix = if img_no > 0 { img_no.to_string() }  else { String::new() };
        let file_name = format!("thumbnails{}.jpg", postfix);
        make_preview_from_range(images, start, end, &file_name);

        if end == images.len() {
            break;
        }
        start = end; 
        end = min(images.len() - start, MAX_TN_COUNT) + start;
        img_no += 1;
    }
}

// A jpg can only have 2^16 rows, so we create multiple thumbnail jpgs if necessary
fn make_preview_from_range(images: &Vec<Image>, start: usize, end: usize, file_name: &String)
{
    let mut buffer: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(THUMBNAIL_SIZE as u32, THUMBNAIL_SIZE * (end - start) as u32);
    for i in start .. end {
        println!("Making thumbnail for {}", &images[i].path);
        let image = if is_jpg(&images[i].path) {
                        read_image(Path::new(&images[i].path))
                    } else {
                        match Command::new("ffmpeg").args(["-y", "-i", &images[i].path,
                                                                "-ss", "00:00:01",
                                                                "-vframes", "1",
                                                                "-update", "true", &TMP_FILE]).output() {
                            Ok(output) => {
                                if !output.status.success() {
                                    eprintln!("Making preview for {} failed with {}\n{}", images[i].path, 
                                                                                          String::from_utf8(output.stdout).unwrap(), 
                                                                                          String::from_utf8(output.stderr).unwrap());
                                    continue;
                                }
                                read_image(Path::new(&TMP_FILE))
                            },
                            Err(e) => {
                                eprintln!("Making preview for {} failed with error {}", &images[i].path, e);
                                continue;
                            }

                    }
        };

        match image {
            Ok(img) => {
                let tn = thumbnail(&img, THUMBNAIL_SIZE as u32, THUMBNAIL_SIZE as u32);
                buffer.copy_from(&tn, 0, (i - start) as u32 * THUMBNAIL_SIZE).expect("Copying bits failed?");

                if is_mp4(&images[i].path) {
                    make_mp4_preview(&images[i]);
                }
            }
            Err(e) => {
                println!("Unable make thumbnail for {} with error {}", &images[i].path, e);
                continue;
            }
        }

    }

    buffer.save(file_name.as_str()).expect("Failed to save thumbnails");
}

fn make_mp4_preview(image: &Image) {
    let p = as_preview(&image.path);
    let path = Path::new(p.as_str());
    let size = image.height as f64 * image.width as f64;
    let preview_percent = (1920.0 * 1080.0) / size;
    if preview_percent < 1.0 {
        let mut buffer = Vec::<u8>::new();
        resize_image(Path::new(TMP_FILE),
                        (image.width as f64 * preview_percent.sqrt()).floor() as u32,
                        (image.height as f64 * preview_percent.sqrt()).floor() as u32,
                        &mut buffer);

        let _ = fs::write(&path, buffer);                                              
    } else {
        let _ = fs::copy(&TMP_FILE, path);
    }
}

pub fn resize_image(path: &Path, width: u32, height: u32, buffer: &mut dyn Write) {
    let image = read_image(path).expect("Unable to read image");
    let mut size_percent = f64::min(width as f64 / image.width() as f64, height as f64 / image.height() as f64);
    
    if size_percent >= 1.0 {
        size_percent = 1.0;
    }

    let resized = resize(&image, (image.width() as f64 * size_percent).floor() as u32,
                                 (image.height() as f64 * size_percent).floor() as u32, FilterType::Gaussian);

    let _ = resized.write_with_encoder(JpegEncoder::new(buffer));

}

use std::collections::HashMap;

use std::env;
use std::fs;
use std::fs::File;

use std::io::{ prelude::*, BufReader };

use std::include_str;
use std::process::{ Command, exit, Output };
use std::str::FromStr;
use std::vec::Vec;

use chrono::DateTime;
use chrono::NaiveDateTime;

use serde_json;

use rust_gallery;

use rust_gallery::MD_FILE;
use rust_gallery::Image;
use rust_gallery::make_preview;

fn main() {
    let mut date_srt = false;
    let mut num_srt = false;

    let args: Vec<String> = env::args().collect();
    for arg in args {
        if arg == "-h" {
            println!("Run from the directory with the media.");
            println!("\tTo sort by filename in date-time format rather than exif use '-d'");
            println!("\tTo sort by filename in numerical format rather than exif use '-n'");
            return;
        }
        if arg == "-d" {
            date_srt = true;
        }
        if arg == "-n" {
            num_srt = true;
        }
    }

    let mut images = match read_exif(date_srt) {
        Ok(i) => i,
        Err(e) => { 
            println!("Unable to parse images {}", e.to_string()); 
            return; 
        }
    };

    if num_srt {
        images.sort_by_key(|i| get_digits(&i.path).parse::<u32>().unwrap());
    } else {
        images.sort_by_key(|i| i.time);
    }

    make_preview(&images);
    
    downscale_videos(&mut images);

    // save metadata
    let _ = fs::write(MD_FILE, serde_json::to_string_pretty(&images).unwrap());

    save_html();
}

fn extract_value(line: &String) -> String {
    let s = line.splitn(5, '\"').collect::<Vec<_>>().get(3).expect("Metadata not well formed").to_string();
    return s;
}

fn load_captions() -> HashMap<String, String> {
    let mut captions = HashMap::new();
    let file = match File::open("captions.txt") {
        Ok(f) => f,
        Err(_) => { 
            // Maybe there's an old metadata file with captions?
            // Read as text because it's simpler than dealing with
            // metadata schema versioning.
            let md = match File::open(MD_FILE) {
                Ok(f) => f,
                Err(_) => {
                    return captions;
                }
            };

            let mut path = String::new();
            let rdr = BufReader::new(md);
            for line in rdr.lines() {
                let l = line.expect("Well formed metadata line");
                if l.starts_with("    \"path") {
                    path = extract_value(&l);
                } else if l.starts_with("    \"caption") {
                    captions.insert(path.clone(), extract_value(&l));
                }
            }
            return captions;
        }
    };

    let reader = BufReader::new(file);
    
    for line in reader.lines() {
        let l = line.expect("Well formed caption line");
        let mut split = l.splitn(2, ' ');
        captions.insert(split.next().expect("Some photo").to_string(), split.next().expect("Some caption").to_string());
    }

    captions
}

fn run(cmd: &str) -> std::io::Result<Output> {
    match Command::new("bash").args(["-c", cmd]).output() {
        Ok(o) => {
            if !o.status.success() {
                let mut error = Vec::<u8>::with_capacity(o.stdout.len() + o.stderr.len());
                error.extend(o.stdout);
                error.extend(o.stderr); 
                return Err(std::io::Error::new(std::io::ErrorKind::Other, String::from_utf8(error).expect("Output not UTF8")));
            }
            Ok(o)
        }
        Err(e) => Err(e)
    }
}

fn get_empty_date() -> NaiveDateTime {
    return DateTime::from_timestamp(0, 0).unwrap().naive_utc();
}

fn assign_date(exiftool_line: &str, image: &mut Image) {
    let date_time_str = &exiftool_line[exiftool_line.rfind(": ").unwrap() + 2 ..];
    match NaiveDateTime::parse_from_str(date_time_str, "%Y:%m:%d %H:%M:%S") {
        Ok(dt) => { image.time = dt; },
        Err(e) => { 
            println!("Unable to parse a date {} for {} with error: {}. Verify photo ordering to determine if it's correct.", date_time_str, image.path, e.to_string());
        }
    };
}

fn assign_date_from_filename(image: &mut Image) {
    let mut date = get_digits(&image.path);
    if date.len() != 14 {
        println!("Date {} is not well formed for {}; should be YYYYmmddHHMMSS", date, image.path);
        exit(1);
    }

    image.time = match NaiveDateTime::parse_from_str(date.as_str(), "%Y%m%d%H%M%S") {
        Ok(dt) => dt,
        Err(e) => {
            // We're going to treat the HMS part (i.e, time) as a counter rather 
            // than HMS. Treat the index as a time.
            let time: u32 = date.split_off(8).parse().expect("Time is not made of digits");
            println!("Treating {} as a counter rather than a time in {} as we get {} otherwise", time, image.path, e);
            let minutes = time / 60;
            let secs = time % 60;

            date = format!("{}00{:0>2}{:0>2}", date, minutes, secs);

            match NaiveDateTime::parse_from_str(date.as_str(), "%Y%m%d%H%M%S") {
                Ok(dt) => dt,
                Err(e) => {
                    println!("Unable to parse a date {} with error: {}", date, e.to_string());
                    exit(1);
                }
            }
        }
    }
}

fn get_digits(s: &String) -> String{
    return s.chars().filter(|c| c.is_digit(10)).collect();
}

fn read_exif(use_fn_date: bool) -> Result<Vec<Image>, String> {
    let captions = load_captions();

    // exiftool seems much more robust and complete than any alternatives, so we spawn
    let cmd = "shopt -s nullglob &&
                exiftool -m -d '%Y:%m:%d %H:%M:%S' -CreateDate -DateTimeOriginal -FileModifyDate -ImageWidth -ImageHeight *.{jpg,JPG,mp4,MP4,mov,MOV,avi,AVI}";
    
    // Run it through bash to get path expansion rather than running exif directly
    let output = match run(cmd) {
        Ok(o) => o,
        Err(e) => return Err(e.to_string())        
    };

    let mut images = Vec::<Image>::new();
    let path_delimiter = "======== ";
    let mut wait_for_next = false;

    for line in String::from_utf8(output.stdout).unwrap().lines() {
        if line.starts_with(path_delimiter) {
            wait_for_next = false;
            let path = line[path_delimiter.len()..].to_string();
            if path.ends_with(".preview.jpg") || path.ends_with(".scaled.mp4") || path.starts_with("thumbnails") {
                let _ = fs::remove_file(&path);
                wait_for_next = true;
                continue;
            }
            let caption = match captions.get(&path) {
                Some(c) => c,
                None => ""
            };
            images.push(Image{  path: path, 
                                caption: caption.to_string(),
                                time: get_empty_date(), 
                                height: 0, 
                                width: 0,
                                mp4_scaled: false });
            continue;
        }
        if wait_for_next {
            continue;
        }

        let index = images.len() - 1;

        if use_fn_date {
            if images[index].time == get_empty_date() {
                assign_date_from_filename(&mut images[index]);
            }            
        }

        if line.starts_with("Create Date") {
            if images[index].time != get_empty_date() {
                continue;
            }            
            assign_date(&line, &mut images[index]);
            continue;
        }
        if line.starts_with("Date/Time Original") {
            if images[index].time != get_empty_date() {
                continue;
            }            
            assign_date(&line, &mut images[index]);
            continue;
        }
        if line.starts_with("File Modification Date") {
            if images[index].time != get_empty_date() {
                continue;
            }            
            assign_date(&line, &mut images[index]);
            continue;
        }
        if line.starts_with("Image Width") {
            images[index].width = u16::from_str(&line[line.rfind(": ").unwrap() + 2 ..]).unwrap();
            continue;
        }
        if line.starts_with("Image Height") {
            images[index].height = u16::from_str(&line[line.rfind(": ").unwrap() + 2 ..]).unwrap();
            continue;
        }
    }

    Ok(images)
}

fn downscale_videos(images: &mut Vec<Image>) {
    for img in images {
        if img.is_mp4() && needs_scaling(img) {
            img.mp4_scaled = true;
            println!("Downscaling {}", img.path);
            let cmd = format!("ffmpeg -y -i {} -vf scale=1920:-2 -c:a copy -c:v libx264 -f mp4 {}.scaled.mp4", img.path, img.path);
            match run(cmd.as_str()) {
                Ok(_) => (),
                Err(e) => {
                    println!("Failed to scale {} with error {}", img.path, e);
                }
            }
        }
    }
}

// Can the video format not be rendered by browsers, or is it too large for normal screens?
fn needs_scaling(image: &Image) -> bool {
    let cmd = format!("ffprobe -v error -select_streams v:0 -show_entries stream=codec_name {}", image.path);
    let output = match run(cmd.as_str()) {
        Ok(o) => o,
        Err(e) => {
            println!("Error getting encoding of {}: {}", image.path, e);
            return false; 
        }        
    };

    let codec = String::from_utf8(output.stdout).expect("There is an encoding");
    if codec.contains("codec_name=hevc") || codec.contains("codec_name=mjpeg") {
        return true;
    }

    image.width > 1920  // we scale to ~ 1920x1080 which is somewhat arbitrary
}

fn save_html() {
    let index = include_str!("../../../html/index.html");
    let _ = fs::write("index.html", index);

    let edit_caption = include_str!("../../../html/edit_caption.js");
    let _ = fs::write("edit_caption.js", edit_caption);
}

use alloc::{borrow::ToOwned, vec::{Vec}};
use alloc::vec;
use anyhow::Error;
use crankstart::{file::FileSystem, log_to_console};
use crankstart_sys::FileOptions;

use crate::dither::calc_z_order;

pub const MAP_WIDTH: u32 = 1024;
pub const MAP_HEIGHT: u32 = 1024;

pub struct Map {
    pub color_altitude: Vec<u32>, //color then altitude
}

fn read_file(path: &str) -> Result<Vec<u8>, Error> {
    let stat = FileSystem::get().stat(path)?;
    let mut buffer: Vec<u8> = vec![0; stat.size as usize];
    let sd_file =
        FileSystem::get().open(path, FileOptions::kFileRead | FileOptions::kFileReadData)?;
    sd_file.read(&mut buffer)?;
    Ok(buffer)
}

pub fn read_image(path: &str) -> Vec<u8> {
    let file_bytes = read_file(path).expect("read_file");
    let file_iter: core::slice::Iter<u8> = file_bytes.iter();
    let mut found_header_0x0a = 0;
    let mut index = 0;
    for byte in file_iter {
        if *byte == 0x0A {
            found_header_0x0a += 1
        }
        if found_header_0x0a == 3 {
            break;
        }
        index += 1;
    }

    log_to_console!("found offset {}", index);
    log_to_console!("image size {}", file_bytes.len() - index);

    file_bytes[index + 1..].to_vec()
}

pub fn load_map(filenames: &str, map: &mut Map) {
    let files: Vec<&str> = filenames.split(';').collect();
    let datac: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[0] + ".pgm"));
    let datah: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[1] + ".pgm"));
    let mut i: usize = 0;
    while i < (MAP_WIDTH * MAP_HEIGHT) as usize {
        let h_i = calc_z_order(i % MAP_WIDTH as usize, i / MAP_WIDTH as usize);
        if h_i % 2 == 0 {
            map.color_altitude[h_i >> 1] |= ((datac[i] as u32) << 8) | datah[i] as u32;
        } else {
            map.color_altitude[h_i >> 1] |= ((datac[i] as u32) << 24) | ((datah[i] as u32) << 16);
        }

        i += 1;
    }
}

use alloc::vec::{Vec};
use alloc::vec;

const DITHER_MATRIX: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const DITHER_MATRIX_256: [u8; 16]  = [0, 127,  31, 159, 191,  63, 223,  95,  47, 175,  15, 143, 239,
       111, 207,  79];

const DITHER_MATRIX_256_2: [u8; 16]  = [0, 128,  32, 159, 191,  64, 223,  96,  48, 175,  16, 143, 239,
112, 207,  80];
       
//todo merge with rendering
pub fn simple_ordered_dither(grayscale: &[u8], output: &mut [u8], x_len: usize, y_len: usize) {

    for x in 0..x_len {
        for y in 0..y_len{
            let offset = (y * x_len) + x;
            let i = x % 4;
            let j = y % 4;
            let dither_offset =  (j * 4) + i;

            let dither_threshold =  DITHER_MATRIX_256_2[dither_offset];
            if grayscale[offset] > dither_threshold {
                output[offset] = 1;
            } else {
                output[offset] = 0;
            }
        }
    }
}

/// Helper to extend images used in testing
pub fn extend_image(input: &[u8], x_len: usize, y_len: usize, output: &mut[u8], out_x_len: usize, out_y_len: usize) {

    for x in 0..x_len {
        for y in 0..y_len{
            let offset_input = (y * x_len) + x;
            let offset_output = (y * out_x_len) + x;

            output[offset_output] = input[offset_input];
        }
    }
}


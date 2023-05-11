/*
const DITHER_MATRIX: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const DITHER_MATRIX_256: [u8; 16] = [
    0, 127, 31, 159, 191, 63, 223, 95, 47, 175, 15, 143, 239, 111, 207, 79,
];
*/

pub const DITHER_MATRIX_256_2: [u8; 16] = [
    0, 128, 32, 159, 191, 64, 223, 96, 48, 175, 16, 143, 239, 112, 207, 80,
];

//http://graphics.stanford.edu/~seander/bithacks.html
const MASKS: [usize; 4] = [0x55555555, 0x33333333, 0x0F0F0F0F, 0x00FF00FF];
const SHIFTS: [usize; 4] = [1, 2, 4, 8];

//Also tested H-curve (not hilbert yet) and was slower
pub fn calc_z_order(x_pos: usize, y_pos: usize) -> usize {
    let mut x = x_pos; // Interleave lower 16 bits of x and y, so the bits of x
    let mut y = y_pos; // are in the even positions and bits from y in the odd;

    x = (x | (x << SHIFTS[3])) & MASKS[3];
    x = (x | (x << SHIFTS[2])) & MASKS[2];
    x = (x | (x << SHIFTS[1])) & MASKS[1];
    x = (x | (x << SHIFTS[0])) & MASKS[0];

    y = (y | (y << SHIFTS[3])) & MASKS[3];
    y = (y | (y << SHIFTS[2])) & MASKS[2];
    y = (y | (y << SHIFTS[1])) & MASKS[1];
    y = (y | (y << SHIFTS[0])) & MASKS[0];

    
    x | (y << 1)
}

pub mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::calc_z_order;

    pub fn test_z_curve() {
        let mut buf: Vec<i32> = vec![-1; 1024 * 1024];
        for x in 0..1024 {
            for y in 0..1024 {
                buf[calc_z_order(x, y)] = (y * 1024 + x) as i32;
            }
        }
        for x in 0..1024 {
            for y in 0..1024 {
                assert_eq!(buf[calc_z_order(x, y)], (y * 1024 + x) as i32);
            }
        }
    }
}

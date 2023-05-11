#![no_std]
/// Code largely based on https://github.com/s-macke/VoxelSpace, some
/// sections (input handling) pretty much unchanged
use alloc::{vec};
use crankstart::{
    geometry::ScreenPoint,
    graphics::{Graphics, LCDColor},
    log_to_console,
};
use crankstart_sys::{LCDSolidColor, PDButtons, LCD_COLUMNS, LCD_ROWSIZE};
use dither::{
    calc_z_order,
    tests::{test_z_curve},
    DITHER_MATRIX_256_2,
};
use map::{Map, MAP_HEIGHT, MAP_WIDTH};

use crate::map::load_map;

use {
    alloc::boxed::Box,
    anyhow::Error,
    crankstart::{crankstart_game, system::System, Game, Playdate},
    crankstart_sys::LCD_ROWS,
    euclid::Trig,
};

extern crate alloc;

pub mod dither;
pub mod map;

struct Camera {
    x: f32,
    y: f32,
    height: f32,
    angle: f32,
    horizon: f32,
}

struct Input {
    forwardbackward: i32,
    leftright: i32,
    updown: i32,
    lookup: bool,
    lookdown: bool,
    keypressed: bool,
}

const HIGH_ALTITUTE_MASK: u32 = 0x00FF0000;
const HIGH_COLOR_MASK: u32 = 0xFF000000;
const LOW_ALTITUTE_MASK: u32 = 0x000000FF;
const LOW_COLOR_MASK: u32 = 0x0000FF00;

const HIGH_ALTITUTE_SHIFT: u32 = 16;
const HIGH_COLOR_SHIFT: u32 = 24;
const LOW_ALTITUTE_SHIFT: u32 = 0;
const LOW_COLOR_SHIFT: u32 = 8;

const VIEW_DISTANCE: f32 = 400.0;

fn handle_input(input: &mut Input) {
    let current: PDButtons;
    let pushed: PDButtons;

    unsafe {
        (current, pushed, _) = System::get().get_button_state().unwrap_unchecked();
    }
    //log_to_console!("pushed {:?}", pushed);
    //log_to_console!("current {:?}", current);

    if ((pushed | current) & PDButtons::kButtonA) == PDButtons::kButtonA {
        input.forwardbackward = 3;
    } else {
        input.forwardbackward = 0;
    }

    if ((pushed | current) & PDButtons::kButtonLeft) == PDButtons::kButtonLeft {
        input.leftright = 1;
    } else if ((pushed | current) & PDButtons::kButtonRight) == PDButtons::kButtonRight {
        input.leftright = -1;
    } else {
        input.leftright = 0;
    }

    if ((pushed | current) & PDButtons::kButtonUp) == PDButtons::kButtonUp {
        input.updown = 1;
    } else if ((pushed | current) & PDButtons::kButtonDown) == PDButtons::kButtonDown {
        input.updown = -1;
    } else {
        input.updown = 0;
    }
}

// Update the camera for next frame. Dependent on keypresses
fn update_camera(input: &mut Input, camera: &mut Camera, time: &mut usize, map: &Map) {
    let current = System::get().get_current_time_milliseconds().unwrap();

    input.keypressed = false;
    if input.leftright != 0 {
        camera.angle += input.leftright as f32 * 0.1 * (current - *time) as f32 * 0.03;
        input.keypressed = true;
    }
    if input.forwardbackward != 0 {
        camera.x -=
            input.forwardbackward as f32 * (camera.angle).sin() * (current - *time) as f32 * 0.03;
        camera.y -=
            input.forwardbackward as f32 * (camera.angle).cos() * (current - *time) as f32 * 0.03;
        input.keypressed = true;
    }
    if input.updown != 0 {
        camera.height += input.updown as f32 * (current - *time) as f32 * 0.03;
        input.keypressed = true;
    }
    if input.lookup {
        camera.horizon += 2. * (current - *time) as f32 * 0.03;
        input.keypressed = true;
    }
    if input.lookdown {
        camera.horizon -= 2. * (current - *time) as f32 * 0.03;
        input.keypressed = true;
    }

    // Collision detection. Don't fly below the surface.
    let h_i = calc_z_order(
        (camera.x as i32 & (MAP_HEIGHT - 1) as i32) as usize,
        (camera.y as i32 & (MAP_WIDTH - 1) as i32) as usize,
    );

    let byte_offset: u32 = if h_i % 2 == 1 {
        HIGH_ALTITUTE_SHIFT
    } else {
        LOW_ALTITUTE_SHIFT
    };
    let byte_mask: u32 = if h_i % 2 == 1 {
        HIGH_ALTITUTE_MASK
    } else {
        LOW_ALTITUTE_MASK
    };

    let altitute: u8;

    unsafe {
        altitute = (((map.color_altitude.get_unchecked(h_i >> 1)) & byte_mask)
            >> byte_offset) as u8;
    }
    if (altitute + 10) > camera.height as u8 {
        camera.height = (altitute + 10) as f32;
    }

    *time = current;
}

const COL_DIV: usize = 2;
const ROW_DIV: usize = 2;
const SCREEN_WIDTH: usize = LCD_COLUMNS as usize / COL_DIV;
const SCREEN_HEIGHT: usize = LCD_ROWS as usize / ROW_DIV;

// ---------------------------------------------
// The main render routine

fn render(map: &Map, camera: &Camera, graphics: &Graphics, input: &Input) {
    let mapwidthperiod = MAP_WIDTH - 1;
    let mapheightperiod = MAP_HEIGHT - 1;

    let sinang = (camera.angle).sin();
    let cosang = (camera.angle).cos();

    let mut i: usize = 0;
    unsafe {
        graphics
            .clear(LCDColor::Solid(LCDSolidColor::kColorWhite))
            .unwrap_unchecked();
    }

    let frame: &mut [u8];
    unsafe {
        frame = graphics.get_frame().unwrap_unchecked();
    }

    while i < SCREEN_WIDTH {
        // Draw from front to back
        let mut z = 1.0;
        let mut hidden_y_at_i = SCREEN_HEIGHT as u16;
        let mut deltaz = 1.;

        let ylean: f32 = -(input.leftright as f32 * (i as f32 - SCREEN_WIDTH as f32 / 2.0)
            / SCREEN_WIDTH as f32)
            * SCREEN_HEIGHT as f32
            / 4.;
        while z < VIEW_DISTANCE {
            // 90 degree field of view
            let mut plx = -cosang * z - sinang * z;
            let mut ply: f32 = sinang * z - cosang * z;

            let prx = cosang * z - sinang * z;
            let pry = -sinang * z - cosang * z;

            let dx = (prx - plx) / SCREEN_WIDTH as f32; //???
            let dy = (pry - ply) / SCREEN_WIDTH as f32;

            plx += camera.x + (i as f32 * dx);
            ply += camera.y + (i as f32 * dy);

            let ply_i32 = ply as i32;
            let plx_i32 = plx as i32;

            let invz = 1. / z * 240.;

            let h_i = calc_z_order(
                (plx_i32 & mapheightperiod as i32) as usize,
                (ply_i32 & mapwidthperiod as i32) as usize,
            );

            let colour_altitude;
            unsafe {
                colour_altitude = *map.color_altitude.get_unchecked(h_i >> 1);
            }

            let byte_offset_colour: u32 = if h_i % 2 == 1 {
                HIGH_COLOR_SHIFT
            } else {
                LOW_COLOR_SHIFT
            };
            let byte_mask_colour: u32 = if h_i % 2 == 1 {
                HIGH_COLOR_MASK
            } else {
                LOW_COLOR_MASK
            };
            let byte_offset_altitude: u32 = if h_i % 2 == 1 {
                HIGH_ALTITUTE_SHIFT
            } else {
                LOW_ALTITUTE_SHIFT
            };
            let byte_mask_altitute: u32 = if h_i % 2 == 1 {
                HIGH_ALTITUTE_MASK
            } else {
                LOW_ALTITUTE_MASK
            };

            let colour: u8 = ((colour_altitude & byte_mask_colour) >> byte_offset_colour) as u8;
            let altitude: u8 =
                ((colour_altitude & byte_mask_altitute) >> byte_offset_altitude) as u8;

            let heightonscreen =
                (((camera.height - altitude as f32) * invz + camera.horizon) + ylean) as u16;

            let mut k = heightonscreen;
            while k < hidden_y_at_i {
                let converted_index = i * 2;
                let converted_k_index = k * 2;

                let offset_buff: usize = (converted_k_index as usize * LCD_ROWSIZE as usize * 8)
                    + converted_index;
                let offset_buff_2: usize =
                    ((converted_k_index + 1) as usize * LCD_ROWSIZE as usize * 8)
                        + converted_index;

                let dither_i = converted_index % 4;
                let dither_i_1 = (converted_index + 1) % 4;

                let dither_j = converted_k_index as usize % 4;
                let dither_j_1 = (converted_k_index + 1) as usize % 4;

                let mut buf = 0xFF;

                let dither_offset = (dither_j * 4) + dither_i;
                let dither_threshold = DITHER_MATRIX_256_2[dither_offset];

                if colour < dither_threshold {
                    buf &= 0x7F_u8.rotate_right((converted_index % 8) as u32);
                }

                let dither_offset = (dither_j * 4) + dither_i_1;
                let dither_threshold = DITHER_MATRIX_256_2[dither_offset];

                if colour < dither_threshold {
                    buf &= 0x7F_u8.rotate_right(((converted_index + 1) % 8) as u32);
                }

                unsafe {
                    if buf != 0xFF {
                        *frame.get_unchecked_mut(offset_buff / 8) &= buf;
                    }
                }

                let mut buf = 0xFF;

                let dither_offset = (dither_j_1 * 4) + dither_i;
                let dither_threshold = DITHER_MATRIX_256_2[dither_offset];

                if colour < dither_threshold {
                    buf &= 0x7F_u8.rotate_right((converted_index % 8) as u32);
                }

                let dither_offset = (dither_j_1 * 4) + dither_i_1;
                let dither_threshold = DITHER_MATRIX_256_2[dither_offset];

                if colour < dither_threshold {
                    buf &= 0x7F_u8.rotate_right(((converted_index + 1) % 8) as u32);
                }
                unsafe {
                    if buf != 0xFF {
                        *frame.get_unchecked_mut(offset_buff_2 / 8) &= buf;
                    }
                }
                k += 1;
            }
            if heightonscreen < hidden_y_at_i {
                hidden_y_at_i = heightonscreen;
            }
            deltaz += 0.04; //0.005
            z += deltaz;
        }
        i += 1;
    }
    unsafe {
        graphics
            .mark_updated_rows(0..=(LCD_ROWS as i32) - 1)
            .unwrap_unchecked();
    }
}

// ---------------------------------------------
// Draw the next frame

fn draw(map: &Map, camera: &mut Camera, input: &mut Input, time: &mut usize, graphics: &Graphics) {
    update_camera(input, camera, time, map);
    render(map, camera, graphics, input);
}

// ---------------------------------------------
// Init routines

fn init() -> State {
    // ---------------------------------------------
    // Viewer information

    let camera = Camera {
        x: 1024.,      // x position on the map
        y: 800.,       // y position on the map
        height: 78.,   // height of the camera
        angle: 0.,     // direction of the camera
        horizon: 100., // horizon position (look up and down)
    };

    // ---------------------------------------------
    // Landscape data

    let mut map = Map {
        color_altitude: vec![0; (1024 * 1024) >> 1], // 1024 * 1024 byte array with height information
    };

    log_to_console!(
        "Map byte offset {:p} {:p}",
        map.color_altitude.first().expect("missing index"),
        map.color_altitude.get(1).expect("missing index")
    );

    // ---------------------------------------------
    // Keyboard and mouse interaction

    let input = Input {
        forwardbackward: 0,
        leftright: 0,
        updown: 0,
        lookup: false,
        lookdown: false,
        keypressed: false,
    };

    let time = System::get().get_current_time_milliseconds().unwrap();

    let mut i: usize = 0;
    while i < ((MAP_WIDTH * MAP_HEIGHT) >> 1) as usize {
        //color to 0xFF, altitute to 0
        map.color_altitude[i] = 0;
        i += 1;
    }

    load_map("C1W;D1", &mut map);

    State {
        start_time: time,
        map,
        camera,
        input,
        graphics: Graphics::get(),
    }
}

fn run(state: &mut State) -> Result<(), Error> {
    //handle events
    handle_input(&mut state.input);
    //draw
    draw(
        &state.map,
        &mut state.camera,
        &mut state.input,
        &mut state.start_time,
        &state.graphics,
    );

    //draw fps
    System::get().draw_fps(0, 0)?;
    Ok(())
}

struct State {
    start_time: usize,
    map: Map,
    camera: Camera,
    input: Input,
    graphics: Graphics,
}

impl State {
    pub fn new(_playdate: &Playdate) -> Result<Box<Self>, Error> {
        log_to_console!("Yay");
        crankstart::display::Display::get().set_refresh_rate(40.0)?;
        Ok(Box::new(init()))
    }
}

impl Game for State {
    fn update(&mut self, _playdate: &mut Playdate) -> Result<(), Error> {
        run(self)?;
        Ok(())
    }
}

crankstart_game!(State);

struct Test {}

impl Test {
    pub fn new(_playdate: &Playdate) -> Result<Box<Self>, Error> {
        log_to_console!("Yay");
        crankstart::display::Display::get().set_refresh_rate(5.0)?;
        Ok(Box::new(Test {}))
    }
}

impl Game for Test {
    fn update(&mut self, _playdate: &mut Playdate) -> Result<(), Error> {
        System::get().draw_fps(0, 0)?;
        test_z_curve();
        Graphics::get()
            .draw_text("Passed test_z_curve", ScreenPoint::new(0, 25))
            .expect("write text");
        Ok(())
    }
}

//crankstart_game!(Test);

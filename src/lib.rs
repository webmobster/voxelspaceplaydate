#![no_std]

use alloc::{borrow::ToOwned, vec, vec::Vec};
use crankstart::log_to_console;
use crankstart_sys::{FileOptions, PDButtons};
use dither::simple_ordered_dither_draw;

use crate::dither::{extend_image};

extern crate alloc;

mod dither;

use {
    alloc::boxed::Box,
    anyhow::Error,
    crankstart::{
        crankstart_game,
        file::FileSystem,
        system::System,
        Game, Playdate,
    },
    crankstart_sys::{LCD_COLUMNS, LCD_ROWS},
    euclid::{Trig},
};

struct Camera {
    x: f32,
    y: f32,
    height: f32,
    angle: f32,
    horizon: f32,
}

struct Map {
    color_altitude: Vec<u32>, //color then altitude
}

struct ScreenData {
    bufarray: Vec<u8>, // color data
    hidden_y: Vec<u16>, // color data
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

const MAP_WIDTH: u32 = 1024;
const MAP_HEIGHT: u32 = 1024;
const MAP_SHIFT: u8 = 10;  // power of two: 2^10 = 1024


const VIEW_DISTANCE: f32 = 400.0;  


fn handle_input(input: &mut Input) {
    let (current, pushed, _) = System::get()
        .get_button_state()
        .expect("Get the button state");
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

fn read_file(path: &str) -> Result<Vec<u8>, Error> {
    let stat = FileSystem::get().stat(path)?;
    let mut buffer = Vec::with_capacity(stat.size as usize);
    buffer.resize(stat.size as usize, 0);
    let sd_file =
        FileSystem::get().open(path, FileOptions::kFileRead | FileOptions::kFileReadData)?;
    sd_file.read(&mut buffer)?;
    Ok(buffer)
}

fn read_image(path: &str) -> Vec<u8> {
    let file_bytes = read_file(path).expect("read_file");
    let mut file_iter: core::slice::Iter<u8> = file_bytes.iter();
    let mut found_header_0x0a = 0;
    let mut index = 0;
    while let Some(byte) = file_iter.next() {
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

fn load_map(filenames: &str, map: &mut Map) {
    //FIXME 65536 colors
    let files: Vec<&str> = filenames.split(";").collect();
    let datac: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[0] + ".pgm"));
    let datah: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[1] + ".pgm"));
    let mut i: usize = 0;
    while i < (MAP_WIDTH * MAP_HEIGHT) as usize {
        //log_to_console!("colour {}, altitute: {}", datac[i], datah[i]);
        map.color_altitude[i >> 1] = ((datac[i] as u32) << 8) | datah[i] as u32 | ((datac[i+1] as u32) << 24) | ((datah[i+1] as u32) << 16);
        //log_to_console!("unpacked colour {}, unpacked altitute: {}", ((map.color_altitude[i] & COLOR_MASK) >> 8) as u8, (map.color_altitude[i] & ALTITUTE_MASK) as u8);

        i += 2;
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
    let mapoffset = ((camera.y as i32 & (MAP_WIDTH - 1) as i32) << MAP_SHIFT)
        + (camera.x as i32 & (MAP_HEIGHT - 1) as i32);

    let byte_offset: u32 = if mapoffset % 2 == 1 { HIGH_ALTITUTE_SHIFT } else { LOW_ALTITUTE_SHIFT};
    let byte_mask: u32 = if mapoffset % 2 == 1 { HIGH_ALTITUTE_MASK } else { LOW_ALTITUTE_MASK};

    let altitute: u8 = ((map.color_altitude[(mapoffset >> 1) as usize] & byte_mask) >> byte_offset) as u8;
    if (altitute  + 10) > camera.height as u8 {
        camera.height = (altitute  + 10) as f32;
    }

    *time = current;
}


// ---------------------------------------------
// Basic screen handling

fn draw_background(screendata: &mut ScreenData) {
    let color = 0xFF;
    let mut i = 0;
    while i < screendata.bufarray.len() {
        screendata.bufarray[i] = color;
        i += 1;
    }
}


const COL_DIV: usize = 2;
const ROW_DIV: usize = 2;
const SCREEN_WIDTH: usize = LCD_COLUMNS as usize /COL_DIV;
const SCREEN_HEIGHT: usize = LCD_ROWS as usize /ROW_DIV;


// Show the back buffer on screen
fn flip(output_image_buffer: &[u8]) {
    simple_ordered_dither_draw(
        output_image_buffer,
        LCD_COLUMNS as usize,
        LCD_ROWS as usize,
        SCREEN_WIDTH,
        SCREEN_HEIGHT
    );
}

// ---------------------------------------------
// The main render routine

fn render(map: &Map, camera: &Camera, screendata: &mut ScreenData) {
    let mapwidthperiod = (MAP_WIDTH - 1) as u32;
    let mapheightperiod = (MAP_HEIGHT - 1) as u32;


    let sinang = (camera.angle).sin();
    let cosang = (camera.angle).cos();

    let mut i: usize = 0;
    while i < SCREEN_WIDTH as usize {
        screendata.hidden_y[i] = SCREEN_HEIGHT as u16;
        //FIXME only use first have off array
        i = i + 1;
    }

    let mut deltaz = 1.;

    // Draw from front to back
    let mut z = 1.0;
    while z < VIEW_DISTANCE as f32 {
        // 90 degree field of view
        let mut plx = -cosang * z - sinang * z;
        let mut ply: f32 = sinang * z - cosang * z;
        let prx = cosang * z - sinang * z;
        let pry = -sinang * z - cosang * z;

        let dx = (prx - plx) / SCREEN_WIDTH as f32; //???
        let dy = (pry - ply) / SCREEN_WIDTH as f32;
        plx += camera.x;
        ply += camera.y;
        let mut i: usize = 0;
        let invz = 1. / z * 240.;

        while i < SCREEN_WIDTH  {
            let ply_u32 = ply as i32;
            let plx_u32 = plx as i32;


            let mapoffset: usize = (((ply_u32 & mapwidthperiod as i32) << MAP_SHIFT) + (plx_u32 & mapheightperiod as i32)) as usize;
            let colour_altitude;
            unsafe {
                colour_altitude = *map.color_altitude.get_unchecked(mapoffset >> 1);
            }

            let byte_offset_colour: u32 = if mapoffset % 2 == 1 { HIGH_COLOR_SHIFT } else { LOW_COLOR_SHIFT};
            let byte_mask_colour: u32 = if mapoffset % 2 == 1 { HIGH_COLOR_MASK } else { LOW_COLOR_MASK};
            let byte_offset_altitude: u32 = if mapoffset % 2 == 1 { HIGH_ALTITUTE_SHIFT } else { LOW_ALTITUTE_SHIFT};
            let byte_mask_altitute: u32 = if mapoffset % 2 == 1 { HIGH_ALTITUTE_MASK } else { LOW_ALTITUTE_MASK};

            let colour: u8 = ((colour_altitude & byte_mask_colour) >> byte_offset_colour) as u8;
            let altitude: u8 = ((colour_altitude & byte_mask_altitute) >> byte_offset_altitude) as u8;


            let heightonscreen =
                ((camera.height - altitude as f32) * invz + camera.horizon) as u16;
      

            let hidden_y_at_i;
            unsafe {
                hidden_y_at_i = *screendata.hidden_y.get_unchecked(i);
            }
            
            let height_on_screen_width = heightonscreen as usize * SCREEN_WIDTH;
            // get offset on screen for the vertical line
            let mut offset = height_on_screen_width + i;
            let mut k = heightonscreen;
            while k < hidden_y_at_i { 
                unsafe {
                    *screendata.bufarray.get_unchecked_mut(offset) = colour;
                }
                offset = offset + (SCREEN_WIDTH  as usize);
                k = k + 1;
            }
            if heightonscreen < hidden_y_at_i {
                unsafe {
                    *screendata.hidden_y.get_unchecked_mut(i) = heightonscreen; 
                }
            }
            plx += dx;
            ply += dy;
            i = i + 1;
        }
        deltaz += 0.04; //0.005
        z += deltaz;
    }
}

// ---------------------------------------------
// Draw the next frame

fn draw(
    map: &Map,
    camera: &mut Camera,
    screendata: &mut ScreenData,
    input: &mut Input,
    time: &mut usize,
) {
    update_camera(input, camera, time, map);
    draw_background(screendata);
    render(map, camera, screendata);
    flip(&screendata.bufarray);
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

    log_to_console!("Map byte offset {:p} {:p}", map.color_altitude.get(0).expect("missing index"), map.color_altitude.get(1).expect("missing index"));

    // ---------------------------------------------
    // Screen data

    let screendata = ScreenData {
        bufarray: vec![0; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize], // color data
        hidden_y: vec![0; SCREEN_WIDTH], // y buffer
    };

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
    while i < ((MAP_WIDTH * MAP_HEIGHT)>>1) as usize {
        //color to 0xFF, altitute to 0
        map.color_altitude[i] = 0xFF00FF00;
        i += 1;
    }

    load_map("C1W;D1", &mut map);

    return State {
        start_time: time,
        map: map,
        camera: camera,
        screendata: screendata,
        input: input,
    };
}

fn run(state: &mut State) -> Result<(), Error> {
    //handle events
    handle_input(&mut state.input);
    //draw
    draw(
        &mut state.map,
        &mut state.camera,
        &mut state.screendata,
        &mut state.input,
        &mut state.start_time,
    );

    //draw fps
    System::get().draw_fps(0, 0)?;
    Ok(())
}

struct State {
    start_time: usize,
    map: Map,
    camera: Camera,
    screendata: ScreenData,
    input: Input,
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

struct DebugState {
    image: Vec<u8>,
    output_image_buffer: Vec<u8>,
}

impl DebugState {
    pub fn new(_playdate: &Playdate) -> Result<Box<Self>, Error> {
        log_to_console!("Yay");
        crankstart::display::Display::get().set_refresh_rate(40.0)?;
        Ok(Box::new(DebugState {
            image: read_image("util_asset/Ordered_4x4_Bayer_matrix_dithering.pgm"),
            output_image_buffer: vec![0; (LCD_COLUMNS * LCD_ROWS) as usize],
        }))
    }
}

impl Game for DebugState {
    fn update(&mut self, _playdate: &mut Playdate) -> Result<(), Error> {
        extend_image(&self.image, 323, 48, &mut self.output_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize);
        simple_ordered_dither_draw(&self.output_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize, LCD_COLUMNS as usize, LCD_ROWS as usize);

        //extend_image(&vec![1; 323*48], 323, 48, &mut self.output_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize);
        //draw_screen(&self.output_image_buffer);

        //let mut a = vec![1; ((LCD_COLUMNS * LCD_ROWS)-LCD_COLUMNS -1) as usize];
        //let mut b = vec![0;LCD_COLUMNS as usize + 1];
        //a.append(&mut b);
        //draw_screen(&a);

        System::get().draw_fps(0, 0)?;
        Ok(())
    }
}

crankstart_game!(State);

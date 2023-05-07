#![no_std]

use alloc::{borrow::ToOwned, vec::Vec, vec};
use crankstart::log_to_console;
use crankstart_sys::{FileOptions, playdate_file, LCD_ROWSIZE, PDButtons};

use crate::dither::{simple_ordered_dither, extend_image};

extern crate alloc;

mod dither;

use {
    alloc::boxed::Box,
    anyhow::Error,
    crankstart::{
        crankstart_game,
        geometry::{ScreenPoint, ScreenVector},
        graphics::{Graphics, LCDColor, LCDSolidColor},
        system::System,
        file::FileSystem,
        Game, Playdate,
    },
    crankstart_sys::{LCD_COLUMNS, LCD_ROWS},
    euclid::{num::Floor, point2, vec2, Trig},
};



struct Camera {
    x: f32,
    y: f32,
    height: f32,
    angle: f32,
    horizon: f32,
    distance: i32,
}

struct Map {
    width: u32,
    height: u32,
    shift: i32,
    altitude: Vec<u8>,
    color: Vec<u8>,
}

struct ScreenData {
    bufarray: Vec<u8>, // color data
    dithered: Vec<u8>, // dithered data

    backgroundcolor: u8,
}

struct Input {
    forwardbackward: i32,
    leftright: i32,
    updown: i32,
    lookup: bool,
    lookdown: bool,
    keypressed: bool,
}

fn handle_input(input: &mut Input) {


    let (current, pushed, _) = System::get().get_button_state().expect("Get the button state");
    log_to_console!("pushed {:?}", pushed);
    log_to_console!("current {:?}", current);


    if ((pushed | current) &  PDButtons::kButtonA) == PDButtons::kButtonA {
        input.forwardbackward = 3;
    } else {
        input.forwardbackward = 0;
    }

    if ((pushed | current) &  PDButtons::kButtonLeft) == PDButtons::kButtonLeft {
        input.leftright = 1;
    } else if ((pushed | current) &  PDButtons::kButtonRight) ==  PDButtons::kButtonRight {
        input.leftright = -1;
    } else {
        input.leftright = 0;
    }

    if ((pushed | current) &  PDButtons::kButtonUp) == PDButtons::kButtonUp {
        input.updown = 1;
    } else if ((pushed | current) &  PDButtons::kButtonDown) ==  PDButtons::kButtonDown {
        input.updown = -1;
    } else {
        input.updown = 0;
    }

    
}


fn read_file(path: &str) -> Result<Vec<u8>, Error> {
    let stat = FileSystem::get().stat(path)?;
    let mut buffer = Vec::with_capacity(stat.size as usize);
    buffer.resize(stat.size as usize, 0);
    let sd_file = FileSystem::get().open(path, FileOptions::kFileRead | FileOptions::kFileReadData)?;
    sd_file.read(&mut buffer)?;   
    Ok(buffer)
}




fn read_image(path: &str) -> Vec<u8> {
    let file_bytes = read_file(path).expect("read_file");
    let mut file_iter: core::slice::Iter<u8> = file_bytes.iter();
    let mut found_header_0x0a = 0;
    let mut index = 0;
    while let Some(byte) = file_iter.next() {
        if *byte == 0x0A{
            found_header_0x0a +=1
        }
        if found_header_0x0a == 3 {
            break;
        }
        index+=1;
    }

    log_to_console!("found offset {}", index);
    log_to_console!("image size {}", file_bytes.len()-index);

    file_bytes[index+1..].to_vec()
}

fn load_map(filenames: &str, map: &mut Map) {
    //FIXME 65536 colors
    let files: Vec<&str> = filenames.split(";").collect();
    let datac: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[0] + ".pgm"));
    let datah: Vec<u8> = read_image(&("processedmaps/".to_owned() + files[1] + ".pgm"));
    let mut i: usize = 0;
    while i < (map.width * map.height) as usize {
        map.color[i] = datac[i];
        map.altitude[i] = datah[i];
        i += 1;
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
    let mapoffset = (((camera.y).floor() as i32 & (map.width - 1) as i32) << map.shift)
        + ((camera.x).floor() as i32 & (map.height - 1) as i32);
    if (map.altitude[mapoffset as usize] + 10) > camera.height as u8 {
        camera.height = (map.altitude[mapoffset as usize] + 10) as f32;
    }

    *time = current;
}

// ---------------------------------------------
// Fast way to draw vertical lines

fn draw_vertical_line(x: usize, mut ytop: f32, ybottom: f32, col: u8, screendata: &mut ScreenData) {
    let screenwidth = LCD_COLUMNS;
    if ytop < 0. {
        ytop = 0.;
    }
    if ytop > ybottom {
        return;
    }

    // get offset on screen for the vertical line
    let mut offset = (ytop as u32 * screenwidth) + x as u32;
    let mut k = ytop;
    while k < ybottom {
        screendata.bufarray[offset as usize] = col;
        offset = offset + screenwidth;
        k = k + 1.;
    }
}

// ---------------------------------------------
// Basic screen handling

fn draw_background(screendata: &mut ScreenData) {
    let color = screendata.backgroundcolor;
    let mut i = 0;
    while i < screendata.bufarray.len() {
        screendata.bufarray[i] = color;
        i += 1;
    }
}

//TODO use bitmapped buffer identical to frame
fn draw_screen(buffer: &[u8]){
    let graphics = Graphics::get();
    let frame = graphics.get_frame().expect("expect to get the frame");

    for y in 0..LCD_ROWS {
        let mut bitpos = 0x80;
        let mut b = 0;
        for x in 0..LCD_COLUMNS {
            let offset: usize = ((y  * LCD_COLUMNS) + x) as usize;
            let offset_buff: usize = ((y  * LCD_ROWSIZE * 8) + x) as usize;

            if buffer[offset] != 0 {
                b = b | bitpos
            }
            bitpos >>= 1;

            if bitpos == 0 {
                frame[offset_buff / 8] = b; //?? why  _not_
                b = 0;
                bitpos = 0x80;
            }
        }
    }

    graphics.mark_updated_rows(0..=(LCD_ROWS as i32) - 1).expect("marked rows");

}

// Show the back buffer on screen
fn flip(output_image_buffer: &[u8], dithered_image_buffer: &mut [u8]) {
    simple_ordered_dither(output_image_buffer, dithered_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize);
    draw_screen(&dithered_image_buffer);
}

// ---------------------------------------------
// The main render routine

fn render(map: &Map, camera: &Camera, screendata: &mut ScreenData) {
    let mapwidthperiod = map.width - 1;
    let mapheightperiod = map.height - 1;

    let screenwidth = LCD_COLUMNS;
    let sinang = (camera.angle).sin();
    let cosang = (camera.angle).cos();

    let mut hiddeny: [f32; LCD_COLUMNS as usize] = [0.0; LCD_COLUMNS as usize];
    let mut i: usize = 0;
    while i < (screenwidth) as usize {
        hiddeny[i] = LCD_ROWS as f32;
        i = i + 1;
    }

    let mut deltaz = 1.;

    // Draw from front to back
    let mut z = 1.0;
    while z < camera.distance as f32 {
        // 90 degree field of view
        let mut plx = -cosang * z - sinang * z;
        let mut ply = sinang * z - cosang * z;
        let prx = cosang * z - sinang * z;
        let pry = -sinang * z - cosang * z;

        let dx = (prx - plx) / screenwidth as f32;//???
        let dy = (pry - ply) / screenwidth as f32;
        plx += camera.x;
        ply += camera.y;
        let invz = 1. / z * 240.;
        let mut i: usize = 0;
        while i < (screenwidth) as usize {
            let mapoffset: usize = (((ply as usize & mapwidthperiod as usize) << map.shift)
                + (plx as usize & mapheightperiod as usize))
                as usize;
            let heightonscreen =
                (camera.height - map.altitude[mapoffset] as f32) * invz + camera.horizon;
            draw_vertical_line(
                i,
                heightonscreen,
                hiddeny[i],
                map.color[mapoffset],
                screendata,
            );
            if heightonscreen < hiddeny[i] {
                hiddeny[i] = heightonscreen;
            }
            plx += dx;
            ply += dy;
            i = i + 1;
        }
        deltaz += 0.005;
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
    flip(&screendata.bufarray, &mut screendata.dithered);
}

// ---------------------------------------------
// Init routines

fn init() -> State{
    // ---------------------------------------------
    // Viewer information

    let camera = Camera {
        x: 1024.,       // x position on the map
        y: 800.,       // y position on the map
        height: 78.,   // height of the camera
        angle: 0.,     // direction of the camera
        horizon: 100., // horizon position (look up and down)
        distance: 400, // distance of map
    };

    // ---------------------------------------------
    // Landscape data

    let mut map = Map {
        width: 1024,
        height: 1024,
        shift: 10,                  // power of two: 2^9 = 1024
        altitude: vec![0; 1024 * 1024], // 1024 * 1024 byte array with height information
        color: vec![0; 1024 * 1024],    // 1024 * 1024 int array with RGB colors
    };

    // ---------------------------------------------
    // Screen data

    let screendata = ScreenData {
        bufarray: vec![0; (LCD_COLUMNS * LCD_ROWS) as usize], // color data
        dithered: vec![0; (LCD_COLUMNS * LCD_ROWS) as usize], // color data
        backgroundcolor: 0xFF,
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
    while i < (map.width * map.height) as usize {
        map.color[i] = 0xFF;
        map.altitude[i] = 0;
        i += 1;
    }

    load_map("C1W;D1", &mut map);

    return State {
        start_time: time,
        map: map,
        camera: camera,
        screendata: screendata,
        input: input
    };
}

fn run(state: &mut State)-> Result<(), Error> {
    //handle events
    handle_input(&mut state.input);
    //draw
    draw(&mut state.map, &mut state.camera, &mut state.screendata, &mut state.input, &mut state.start_time);

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
        crankstart::display::Display::get().set_refresh_rate(20.0)?;
        Ok(Box::new(init()))
    }
}

impl Game for State {
    fn update(&mut self, _playdate: &mut Playdate) -> Result<(), Error> {
        run(self)?;
        log_to_console!("Done iter");
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
        crankstart::display::Display::get().set_refresh_rate(20.0)?;
        Ok(Box::new(DebugState{image: read_image("util_asset/Ordered_4x4_Bayer_matrix_dithering.pgm"), output_image_buffer: vec![0; (LCD_COLUMNS * LCD_ROWS) as usize]}))
    }
}

impl Game for DebugState {
    fn update(&mut self, _playdate: &mut Playdate) -> Result<(), Error> {
        //extend_image(&self.image, 323, 48, &mut self.output_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize);
        //let dither = simple_ordered_dither(&self.output_image_buffer, LCD_COLUMNS as usize, LCD_ROWS as usize);
        //draw_screen(&dither);

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
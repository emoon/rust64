// VIC-II
extern crate sdl2;
use c64::memory;
use c64::cpu;
use std::cell::RefCell;
use std::rc::Rc;
use std::num::Wrapping;
use c64;
use utils;

// preferences (to be modifiable)
static SKIP_FRAMES: u16 = 2;

pub type VICShared = Rc<RefCell<VIC>>;

// number of rasterlines for PAL (0x138)
static NUM_RASTERLINES: u16 = 312;

static FIRST_DISP_LINE: u16 = 0x10;
static LAST_DISP_LINE: u16 = 0x11f;

// first and last possible lines for bad lines
static FIRST_DMA_LINE: u16 = 0x30;
static LAST_DMA_LINE: u16  = 0xF7;

static ROW25_YSTART: u16 = 0x33;
static ROW25_YSTOP:  u16 = 0xFB;
static ROW24_YSTART: u16 = 0x37;
static ROW24_YSTOP:  u16 = 0xF7;

// action to perform on specific VIC events (write, raster irq...)
pub enum VICCallbackAction
{
    None,
    TriggerVICIrq,
    ClearVICIrq,
}

pub struct VIC
{
    mem_ref: Option<memory::MemShared>,
    cpu_ref: Option<cpu::CPUShared>,

    pub window_buffer: Vec<u32>,
    
    irq_flag: u8,
    irq_mask: u8,
    
    matrix_line: [u8; 40], // video line buffer, read in bad lines
    color_line: [u8; 40],  // color line buffer, read in bad lines
    
    pub last_byte: u8,       // last byte read by VIC
    screen_chunk_offset: usize, // current offset from screen start
    line_start_offset: usize,   // offset to the next line start on screen
    fg_mask_offset: usize,      // offset in fg mask for sprite-gfx collisions and prios
    raster_x: u16,       // raster x position
    raster_cnt: u16,     // raster line counter (current raster line)
    raster_irq: u16,     // raster interrupt line
    dy_start: u16,       // border logic helper values
    dy_stop: u16,
    row_cnt: u16,        // row counter
    video_cnt: u16,      // video counter
    video_cnt_base: u16, // video counter base
    x_scroll: u16,
    y_scroll: u16,
    cia_vabase: u16,

    curr_cycle: u16,     // current cycle
    display_mode: u16,   // current display mode
    bad_lines_on: bool,
    lp_triggered: bool,  // lightpen irq triggered
    mc: [u16; 8],        // sprite data counters
    mc_base: [u16; 8],   // sprite data counter bases
    display_state: bool, // true: display state; false: idle state
    border_on: bool,     // upper/lower border on
    ud_border_on: bool,
    frame_skipped: bool, // frame is being skipped
    is_bad_line: bool,
    draw_this_line: bool,
    ml_idx: usize,         // matrix/color line index
    skip_cnt: u16,      // frame skipping counter
    mx: Vec<u16>,       // special register: x position of sprites

    trigger_vblank: bool,
    border_on_sample: [bool; 5],  // samples of border state at cycles 1, 17, 18, 56, 57)
    fg_mask_buffer: [u8; c64::SCREEN_WIDTH/8],
    border_color_sample: [u8; c64::SCREEN_WIDTH/8],
    matrix_base: u16,
    char_base: u16,
    bitmap_base: u16,
    refresh_cnt: u8,    // refresh counter
    sprite_y_exp: u8,   // sprite y expansion flipflops
    sprite_dma_on: u8, // sprite ON flags
    sprite_display_on: u8, // sprite display flags
    sprite_draw: u8,       // draw sprite in this line
    gfx_data: u8,
    char_data: u8,
    color_data: u8,
    last_char_data: u8,
    first_ba_cycle: u32,
}

impl VIC
{    
    //pub fn new_shared(renderer: &sdl2::render::Renderer) -> VICShared
    pub fn new_shared() -> VICShared
    {
        Rc::new(RefCell::new(VIC
        {
            mem_ref: None,
            cpu_ref: None,
            window_buffer: vec![0; c64::SCREEN_WIDTH * c64::SCREEN_HEIGHT],
            irq_flag: 0,
            irq_mask: 0,
            matrix_line: [0; 40],
            color_line: [0; 40],
            last_byte: 0,
            screen_chunk_offset: 0,
            line_start_offset: 0,
            fg_mask_offset: 0,
            raster_x: 0,
            raster_cnt: NUM_RASTERLINES - 1,
            row_cnt: 7,
            raster_irq: 0,
            dy_start: 0,
            dy_stop: 0,
            video_cnt: 0,
            video_cnt_base: 0,
            x_scroll: 0,
            y_scroll: 0,
            cia_vabase: 0,
            curr_cycle: 1,
            display_mode: 0,
            bad_lines_on: false,
            lp_triggered: false,
            mc: [0; 8],
            mc_base: [0; 8],
            display_state: false,
            border_on: false,
            ud_border_on: false,
            frame_skipped: false,
            is_bad_line: false,
            draw_this_line: false,
            ml_idx: 0,
            skip_cnt: 1,
            mx: vec![0; 8],
            trigger_vblank: false,
            border_on_sample: [false; 5],
            fg_mask_buffer: [0; c64::SCREEN_WIDTH/8],
            border_color_sample: [0; c64::SCREEN_WIDTH / 8],
            matrix_base: 0,
            char_base: 0,
            bitmap_base: 0,
            refresh_cnt: 0,
            sprite_y_exp: 0,
            sprite_dma_on: 0,
            sprite_display_on: 0,
            sprite_draw: 0,
            gfx_data: 0,
            char_data: 0,
            color_data: 0,
            last_char_data: 0,
            first_ba_cycle: 0,
        }))
    }
    
    pub fn set_references(&mut self, memref: memory::MemShared, cpuref: cpu::CPUShared)
    {
        self.mem_ref = Some(memref);
        self.cpu_ref = Some(cpuref);
    }
    
    pub fn read_register(&self, addr: u16) -> u8
    {
        match addr
        {
            0xD011 => {
                let curr_val = as_ref!(self.mem_ref).get_ram_bank(memory::MemType::IO).read(addr);
                // bit 7 in $d011 is bit 8 of $d012
                (curr_val & 0x7F) | ((self.raster_cnt & 0x100) >> 1) as u8
            },
            0xD012          => self.raster_cnt as u8,
            0xD019          => self.irq_flag | 0x70,
            0xD01A          => self.irq_mask | 0xF0,
            0xD040...0xD3FF => self.read_register(0xD000 + (addr % 0x0040)),
            _               => as_ref!(self.mem_ref).get_ram_bank(memory::MemType::IO).read(addr)
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8, on_vic_write: &mut VICCallbackAction)
    {
        match addr
        {
            0xD000...0xD00E =>
            {
                if (addr % 2) == 0
                {
                    let idx = ((addr % 0x000F) >> 1) as usize;
                    self.mx[idx] = (self.mx[idx] & 0xFF00) | value as u16;
                    as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, self.mx[idx] as u8);
                }
                else
                {
                    as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
                }
            },
            0xD010 =>
            {
                let mut j = 1;
                
                for i in 0..8
                {
                    if (value & j) != 0
                    {
                        self.mx[i] |= 0x100;
                    }
                    else
                    {
                        self.mx[i] &= 0x00FF;
                    }

                    j <<= 1;
                }
                
                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD011 =>
            {
                self.y_scroll = (value & 7) as u16;

                let new_raster_irq = (self.raster_irq & 0xFF) | ((0x80 & value as u16) << 1);
                if (self.raster_irq != new_raster_irq) && (self.raster_cnt == new_raster_irq)
                {
                    *on_vic_write = self.raster_irq();
                }

                self.raster_irq = new_raster_irq;

                if (value & 8) != 0
                {
                    self.dy_start = ROW25_YSTART;
                    self.dy_stop = ROW25_YSTOP;
                }
                else
                {
                    self.dy_start = ROW24_YSTART;
                    self.dy_stop = ROW24_YSTOP;
                }

                if (self.raster_cnt == 0x30) && ((value & 0x10) != 0)
                {
                    self.bad_lines_on = true;
                }

                self.is_bad_line = (self.raster_cnt >= FIRST_DMA_LINE) &&
                                   (self.raster_cnt <= LAST_DMA_LINE) &&
                                   ((self.raster_cnt & 7) == self.y_scroll) && self.bad_lines_on;
                let ctrl2 = self.read_register(0xD016);
                self.display_mode = (((value & 0x60) | (ctrl2 & 0x10)) >> 4) as u16;
                
                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD012 =>
            {
                let new_raster_irq = (self.raster_irq & 0xFF00) | value as u16;

                if (self.raster_irq != new_raster_irq) && (self.raster_cnt == new_raster_irq)
                {
                    *on_vic_write = self.raster_irq();
                }

                self.raster_irq = new_raster_irq;
                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD016 =>
            {
                let ctrl1 = self.read_register(0xD011);
                self.x_scroll = (value & 7) as u16;
                self.display_mode = (((ctrl1 & 0x60) | (value & 0x10)) >> 4) as u16;

                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD017 =>
            {
                self.sprite_y_exp |= !value;
                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD018 =>
            {
                self.matrix_base = ((value & 0xF0) as u16) << 6;
                self.char_base   = ((value & 0x0E) as u16) << 10;
                self.bitmap_base = ((value & 0x08) as u16) << 10;
                
                as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value);
            },
            0xD019 =>
            {
                self.irq_flag = self.irq_flag & (!value & 0x0F);
                
                if (self.irq_flag & self.irq_mask) != 0
                {
                    self.irq_flag |= 0x80;
                }
                else
                {
                    // normally we'd dereference the cpu directly but in Rust
                    // it's not possible due to RefCell already being borrowed (call by CPU)
                    *on_vic_write = VICCallbackAction::TriggerVICIrq;
                }
            },
            0xD01A =>
            {
                self.irq_mask = value & 0x0F;

                if (self.irq_flag & self.irq_mask) != 0
                {
                    self.irq_flag |= 0x80;
                    *on_vic_write = VICCallbackAction::TriggerVICIrq;
                }
                else
                {
                    self.irq_flag &= 0x7F;
                    *on_vic_write = VICCallbackAction::ClearVICIrq;
                }
            },
            0xD040...0xD3FF => { self.write_register(0xD000 + (addr % 0x0040), value, on_vic_write); },
            _ => as_mut!(self.mem_ref).get_ram_bank(memory::MemType::IO).write(addr, value),
        }
    }
    
    pub fn trigger_lp_irq(&mut self)
    {
        // lightpen triggers only once per frame
        if !self.lp_triggered
        {
            self.lp_triggered = true;
            
            let lpx = self.raster_x >> 1;
            let lpy = self.raster_cnt;
            
            self.irq_flag |= 0x08;
            if (self.irq_mask & 0x08) != 0
            {
                self.irq_flag |= 0x80;
                as_mut!(self.cpu_ref).trigger_vic_irq();
            }

            let mut vicwrite: VICCallbackAction = VICCallbackAction::None;
            self.write_register(0xD013, lpx as u8, &mut vicwrite);
            self.write_register(0xD014, lpy as u8, &mut vicwrite);
        }
    }

    pub fn on_va_change(&mut self, new_va: u8)
    {
        self.cia_vabase = (new_va as u16) << 14;
        let vbase = self.read_register(0xD018);
        let mut vicwrite: VICCallbackAction = VICCallbackAction::None;
        self.write_register(0xD018, vbase, &mut vicwrite);
    }

    pub fn raster_irq(&mut self) -> VICCallbackAction
    {
        self.irq_flag |= 0x01;
 
        if (self.irq_mask & 0x01) != 0
        {
            self.irq_flag |= 0x80;

            // TODO: when the time is right check if this works correctly (irq should be triggered here)
            //as_mut!(self.cpu_ref).trigger_vic_irq();
            VICCallbackAction::TriggerVICIrq
        }
        else
        {
            VICCallbackAction::None
        }
    }

    pub fn read_byte(&mut self, addr: u16) -> u8
    {
        let va = addr | self.cia_vabase;

        if (va & 0x7000) == 0x1000
        {
            let addr = 0xD000 + (va & 0x0FFF);
            self.last_byte = as_mut!(self.mem_ref).get_rom_bank(memory::MemType::CHARGEN).read(addr);
        }
        else
        {
            self.last_byte = as_mut!(self.mem_ref).get_ram_bank(memory::MemType::RAM).read(va);
        }

        self.last_byte
    }

    pub fn matrix_access(&mut self, c64_cycle_cnt: u32)
    {
        if as_ref!(self.cpu_ref).ba_low
        {
            if (c64_cycle_cnt - self.first_ba_cycle) < 3
            {
                self.color_line[self.ml_idx]  = 0xFF;
                self.matrix_line[self.ml_idx] = 0xFF;
            }
            else
            {
                let addr = (self.video_cnt & 0x03FF) | self.matrix_base;
                self.matrix_line[self.ml_idx] = self.read_byte(addr);

                // assign value from color ram
                self.color_line[self.ml_idx] = as_ref!(self.mem_ref).get_ram_bank(memory::MemType::IO).read(0xD800 + (addr & 0x03FF));
            }
        }
    }

    pub fn graphics_access(&mut self)
    {
        let ctrl1 = self.read_register(0xD011);
        
        if self.display_state
        {
            let mut addr: u16;

            if (ctrl1 & 0x20) != 0 // bitmap
            {
                addr = ((self.video_cnt & 0x03FF) << 3) | self.bitmap_base | self.row_cnt;
            }
            else // text
            {
                addr = ((self.matrix_line[self.ml_idx] as u16) << 3) | self.char_base | self.row_cnt;
            }

            if (ctrl1 & 0x40) != 0 // ECM
            {
                addr &= 0xF9FF;
            }

            self.gfx_data = self.read_byte(addr);
            self.char_data = self.matrix_line[self.ml_idx];
            self.color_data = self.color_line[self.ml_idx];

            self.ml_idx += 1;
            self.video_cnt += 1;
        }
        else
        {
            // display is off
            self.gfx_data = self.read_byte(if (ctrl1 & 0x40) != 0 { 0x39FF } else { 0x3FFF });
            self.char_data = 0;
            self.color_data = 0;
        }
    }

    pub fn draw_background(&mut self)
    {
        let dst_color: u8;

        if !self.draw_this_line { return }
        
        match self.display_mode
        {
            // standard text, multicolor text, multicolor bitmap
            0 | 1 | 3 => {
                dst_color = self.read_register(0xD021);
            },
            // standard bitmap
            2 => {
                dst_color = self.last_char_data;
            },
            // ECM text
            4 => {
                if (self.last_char_data & 0x80) != 0
                {
                    if (self.last_char_data & 0x40) != 0
                    {
                        dst_color = self.read_register(0xD024);
                    }
                    else
                    {
                        dst_color = self.read_register(0xD023);
                    }
                }
                else
                {
                    if (self.last_char_data & 0x40) != 0
                    {
                        dst_color = self.read_register(0xD022);
                    }
                    else
                    {
                        dst_color = self.read_register(0xD021);
                    }
                }
            },
            _ => dst_color = 0,
        }

        let color_rgba = utils::fetch_c64_color_rgba(dst_color);
        utils::memset8(&mut self.window_buffer, self.screen_chunk_offset, color_rgba);
    }
    
    pub fn draw_graphics(&mut self)
    {
        if !self.draw_this_line { return }
        
        if self.ud_border_on
        {
            self.draw_background();
            return
        }

        let mut dst_color = [0;4];

        match self.display_mode
        {
            0 => { // standard text
                dst_color[0] = self.read_register(0xD021);
                dst_color[1] = self.color_data;
                self.draw_std(&dst_color);
            },
            1 => { // multicolor text
                if (self.color_data & 8) != 0
                {
                    dst_color[0] = self.read_register(0xD021);
                    dst_color[1] = self.read_register(0xD022);
                    dst_color[2] = self.read_register(0xD023);
                    dst_color[3] = self.color_data & 7;
                    self.draw_multi(&dst_color);
                }
                else
                {
                    dst_color[0] = self.read_register(0xD021);
                    dst_color[1] = self.color_data;
                    self.draw_std(&dst_color);
                }
            },
            2 => { // standard bitmap
                dst_color[0] = self.char_data;
                dst_color[1] = self.char_data >> 4;
                self.draw_std(&dst_color);
            },
            3 => { // multicolor bitmap
                dst_color[0] = self.read_register(0xD021);
                dst_color[1] = self.char_data >> 4;
                dst_color[2] = self.char_data;
                dst_color[3] = self.color_data;
                self.draw_multi(&dst_color);
            },
            4 => { // ECM text
                if (self.char_data & 0x80) != 0
                {
                    if (self.char_data & 0x40) != 0
                    {
                        dst_color[0] = self.read_register(0xD024);
                    }
                    else
                    {
                        dst_color[0] = self.read_register(0xD023);
                    }
                }
                else
                {
                    if (self.char_data & 0x40) != 0
                    {
                        dst_color[0] = self.read_register(0xD022);
                    }
                    else
                    {
                        dst_color[0] = self.read_register(0xD021);
                    }
                }

                dst_color[1] = self.color_data;
                self.draw_std(&dst_color);
            },
            5 => { // invalid multicolor text
                utils::memset8(&mut self.window_buffer, self.screen_chunk_offset + self.x_scroll as usize, 0);

                if (self.color_data & 8) != 0
                {
                    self.fg_mask_buffer[self.fg_mask_offset  ] |= ((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) >> self.x_scroll;
                    self.fg_mask_buffer[self.fg_mask_offset+1] |= ((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) << (8 - self.x_scroll);
                }
                else
                {
                    self.fg_mask_buffer[self.fg_mask_offset  ] |= self.gfx_data >> self.x_scroll;
                    self.fg_mask_buffer[self.fg_mask_offset+1] |= self.gfx_data << (7 - self.x_scroll);
                }
            },
            6 => { // invalid standard bitmap
                utils::memset8(&mut self.window_buffer, self.screen_chunk_offset + self.x_scroll as usize, 0);
                self.fg_mask_buffer[self.fg_mask_offset  ] |= self.gfx_data >> self.x_scroll;
                self.fg_mask_buffer[self.fg_mask_offset+1] |= self.gfx_data << (7 - self.x_scroll);
            },
            7 => { // invalid multicolor bitmap
                utils::memset8(&mut self.window_buffer, self.screen_chunk_offset + self.x_scroll as usize, 0);
                self.fg_mask_buffer[self.fg_mask_offset  ] |= ((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) >> self.x_scroll;
                self.fg_mask_buffer[self.fg_mask_offset+1] |= ((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) << (8 - self.x_scroll);
            },
            _ => panic!("Unknown display mode for drawing graphics!"),
        }
    }

    /* *** helper functions for draw_graphics *** */
    fn draw_std(&mut self, color: &[u8])
    {
        let screen_pos = self.screen_chunk_offset + self.x_scroll as usize;
        
        self.fg_mask_buffer[self.fg_mask_offset     ] |= self.gfx_data >> self.x_scroll;
        self.fg_mask_buffer[self.fg_mask_offset + 1 ] |= self.gfx_data << (7 - self.x_scroll);

        let mut data = self.gfx_data;
        self.window_buffer[screen_pos + 7] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 6] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 5] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 4] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 3] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 2] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos + 1] = utils::fetch_c64_color_rgba(color[(data & 1) as usize]); data >>= 1;
        self.window_buffer[screen_pos    ] = utils::fetch_c64_color_rgba(color[data as usize]);
    }

    fn draw_multi(&mut self, color: &[u8])
    {
        let screen_pos = self.screen_chunk_offset + self.x_scroll as usize;

        self.fg_mask_buffer[self.fg_mask_offset  ] |= ((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) >> self.x_scroll;
        self.fg_mask_buffer[self.fg_mask_offset+1] |= ((((self.gfx_data & 0xAA) | (self.gfx_data & 0xAA) >> 1) as u16) << (8 - self.x_scroll)) as u8;

        let mut data = self.gfx_data;
        self.window_buffer[screen_pos + 7] = utils::fetch_c64_color_rgba(color[(data & 3) as usize]); data >>= 2;
        self.window_buffer[screen_pos + 6] = self.window_buffer[screen_pos + 7];
        self.window_buffer[screen_pos + 5] = utils::fetch_c64_color_rgba(color[(data & 3) as usize]); data >>= 2;
        self.window_buffer[screen_pos + 4] = self.window_buffer[screen_pos + 5];
        self.window_buffer[screen_pos + 3] = utils::fetch_c64_color_rgba(color[(data & 3) as usize]); data >>= 2;
        self.window_buffer[screen_pos + 2] = self.window_buffer[screen_pos + 3];
        self.window_buffer[screen_pos + 1] = utils::fetch_c64_color_rgba(color[(data as usize)]);
        self.window_buffer[screen_pos    ] = self.window_buffer[screen_pos + 1];
    }
    
    pub fn draw_sprites(&self)
    {
        // TODO
    }


    /* ***helper functions *** */

    fn set_ba_low(&mut self, c64_cycle_cnt: u32)
    {
        if !as_mut!(self.cpu_ref).ba_low
        {
            self.first_ba_cycle = c64_cycle_cnt;
            as_mut!(self.cpu_ref).ba_low = true;
        }   
    }

    fn display_if_bad_line(&mut self)
    {
        if self.is_bad_line
        {
            self.display_state = true;
        }
    }

    fn fetch_if_bad_line(&mut self, c64_cycle_cnt: u32)
    {
        if self.is_bad_line
        {
            self.display_state = true;
            self.set_ba_low(c64_cycle_cnt);
        }
    }

    fn rc_if_bad_line(&mut self, c64_cycle_cnt: u32)
    {
        if self.is_bad_line
        {
            self.display_state = true;
            self.row_cnt = 0;
            self.set_ba_low(c64_cycle_cnt);
        }
    }

    fn idle_access(&mut self)
    {
        self.read_byte(0x3FFF);
    }

    fn refresh_access(&mut self)
    {
        let ref_cnt = self.refresh_cnt as u16;
        self.read_byte(0x3F00 | ref_cnt);
        self.refresh_cnt = (Wrapping(self.refresh_cnt) - Wrapping(1)).0;
    }

    fn check_sprite_dma(&mut self)
    {
        // TODO
        for i in 0..8 { self.mc_base[i] = 0; }
    }

    fn sprite_ptr_access(&mut self, num: usize)
    {
        // TODO
    }

    fn sprite_data_access(&mut self, num: usize, bytenum: usize)
    {
        // TODO
    }

    fn sample_border(&mut self)
    {
        if self.draw_this_line == true
        {
            if self.border_on == true
            {
                self.border_color_sample[(self.curr_cycle-13) as usize] = self.read_register(0xD020);
            }
            
            self.screen_chunk_offset += 8;
            self.fg_mask_offset +=1;
        }
    }
    
    /* *** main VIC-II loop *** */
    // returns true if VBlank is to be triggered
    pub fn update(&mut self, c64_cycle_cnt: u32, should_trigger_vblank: &mut bool) -> bool
    {
        let mut mask: u8;
        let mut line_finished = false;

        match self.curr_cycle
        {
            // fetch sprite pointer 3, inc raster counter, trigger raster irq,
            // test for bad line, reset BA if sprites 3 and 4 are off, read data of sprite 3
            1 => {
                if self.raster_cnt == (NUM_RASTERLINES - 1)
                {
                    self.trigger_vblank = true;
                }
                else
                {
                    self.raster_cnt += 1;

                    if self.raster_cnt == self.raster_irq
                    {
                        let ri = self.raster_irq();

                        match ri
                        {
                            VICCallbackAction::TriggerVICIrq => as_mut!(self.cpu_ref).trigger_vic_irq(),
                            _ => (),
                        }
                    }
                    
                    if self.raster_cnt == 0x30
                    {
                        self.bad_lines_on = (self.read_register(0xD011) & 0x10) != 0;
                    }

                    self.is_bad_line = (self.raster_cnt >= FIRST_DMA_LINE) &&
                                       (self.raster_cnt <= LAST_DMA_LINE)  &&
                                       ((self.raster_cnt & 7) == self.y_scroll) &&
                                        self.bad_lines_on;

                    self.draw_this_line = (self.raster_cnt >= FIRST_DISP_LINE) &&
                                          (self.raster_cnt <= LAST_DISP_LINE) && !self.frame_skipped;       
                }

                self.border_on_sample[0] = self.border_on;

                self.sprite_ptr_access(3);
                self.sprite_data_access(3, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x18) == 0
                {
                    as_ref!(self.cpu_ref).ba_low = false;
                }
            },
            // set BA for sprite 5, read data of sprite 3
            2 => {
                if self.trigger_vblank
                {
                    self.raster_cnt = 0;
                    self.video_cnt_base = 0;

                    self.refresh_cnt = 0xFF;
                    self.lp_triggered = false;
                    self.trigger_vblank = false;

                    self.skip_cnt -= 1;
                    self.frame_skipped = self.skip_cnt == 0;

                    if self.frame_skipped
                    {
                        self.skip_cnt = SKIP_FRAMES;
                    }
                    
                    // trigger VBlank here
                    *should_trigger_vblank = true;
                    
                    self.line_start_offset = 0;
                    
                    if self.raster_irq == 0
                    {
                        let ri = self.raster_irq();
                        
                        match ri
                        {
                            VICCallbackAction::TriggerVICIrq => as_mut!(self.cpu_ref).trigger_vic_irq(),
                            _ => (),
                        }
                    }
                }

                self.screen_chunk_offset = self.line_start_offset;
                self.fg_mask_offset = 0;
                self.fg_mask_buffer = [0; c64::SCREEN_WIDTH/8];
                
                self.sprite_data_access(3, 1);
                self.sprite_data_access(3, 2);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x20) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // fetch sprite pointer 4, reset BA if sprite 4 and 5 are off
            3 => {
                self.sprite_ptr_access(4);
                self.sprite_data_access(4, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x30) == 0
                {
                    as_mut!(self.cpu_ref).ba_low = false;
                }
            },
            // set BA for sprite 6, read data of sprite 4
            4 => {
                self.sprite_data_access(4, 1);
                self.sprite_data_access(4, 2);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x40) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // fetch sprite pointer 5, reset BA if sprite 5 and 6 are off
            5 => {
                self.sprite_ptr_access(5);
                self.sprite_data_access(5, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x60) == 0
                {
                    as_mut!(self.cpu_ref).ba_low = false;
                }
            },
            // set BA for sprite 7, read data of sprite 5
            6 => {
                self.sprite_data_access(5, 1);
                self.sprite_data_access(5, 2);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x80) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // fetch sprite pointer 6, reset BA if sprite 6 and 7 are off
            7 => {
                self.sprite_ptr_access(6);
                self.sprite_data_access(6, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0xC0) == 0
                {
                    as_mut!(self.cpu_ref).ba_low = false;
                }
            },
            // read data of sprite 6
            8 => {
                self.sprite_data_access(6, 1);
                self.sprite_data_access(6, 2);
                self.display_if_bad_line();
            },
            // fetch sprite pointer 7, reset BA if sprite 7 are off
            9 => {
                self.sprite_ptr_access(7);
                self.sprite_data_access(7, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x80) == 0
                {
                    as_mut!(self.cpu_ref).ba_low = false;
                }
            },
            // read data of sprite 7
            10 => {
                self.sprite_data_access(7, 1);
                self.sprite_data_access(7, 2);
                self.display_if_bad_line();
            },
            // refresh, reset BA
            11 => {
                self.refresh_access();
                self.display_if_bad_line();
                as_mut!(self.cpu_ref).ba_low = false;
            },
            // refresh, turn on matrix access if bad line
            12 => {
                self.refresh_access();
                self.fetch_if_bad_line(c64_cycle_cnt);
            },
            // refresh, turn on matrix access if bad line, reset raster_x, graphics display starts here
            13 => {
                self.draw_background();
                self.sample_border();
                self.refresh_access();
                self.fetch_if_bad_line(c64_cycle_cnt);
                self.raster_x = 0xFFFC;
            },
            // refresh, reset video counter, turn on matrix access and reset row counter if bad line
            14 => {
                self.draw_background();
                self.sample_border();
                self.refresh_access();
                self.rc_if_bad_line(c64_cycle_cnt);
                self.video_cnt = self.video_cnt_base;
            },
            // refresh, matrix access, inc mc_base by if if y expansion is set
            15 => {
                self.draw_background();
                self.sample_border();
                self.refresh_access();
                self.fetch_if_bad_line(c64_cycle_cnt);

                for i in 0..8
                {
                    if (self.sprite_y_exp & (1 << i)) != 0
                    {
                        self.mc_base[i] += 2;
                    }
                }
                
                self.ml_idx = 0;
                self.matrix_access(c64_cycle_cnt);
            },
            // graphics access, matrix access, inc mc_base by 1 if y expansion is set
            16 => {
                self.draw_background();
                self.sample_border();
                self.graphics_access();
                self.fetch_if_bad_line(c64_cycle_cnt);

                mask = 1;

                for i in 0..8
                {
                    if (self.sprite_y_exp & mask) != 0
                    {
                        self.mc_base[i] += 1;
                    }

                    if (self.mc_base[i] & 0x3F) == 0x3F
                    {
                        self.sprite_dma_on &= !mask;
                    }

                    mask <<= 1;
                }

                self.matrix_access(c64_cycle_cnt);
            },
            // graphics access, matrix access, turn off border in 40 column mode,
            // display window starts here
            17 => {
                let ctrl1 = self.read_register(0xD011);
                let ctrl2 = self.read_register(0xD016);

                if (ctrl2 & 8) != 0
                {
                    if self.raster_cnt == self.dy_stop
                    {
                        self.ud_border_on = true;
                    }
                    else
                    {
                        if (ctrl1 & 0x10) != 0
                        {
                            if self.raster_cnt == self.dy_start
                            {
                                self.border_on = false;
                                self.ud_border_on = false;
                            }
                            else
                            {
                                if !self.ud_border_on
                                {
                                    self.border_on = false;
                                }
                            }
                        }
                        else
                        {
                            if !self.ud_border_on
                            {
                                self.border_on = false;
                            }
                        }
                    }
                }

                self.border_on_sample[1] = self.border_on;

                self.draw_background();
                self.draw_graphics();
                self.sample_border();
                self.graphics_access();
                self.fetch_if_bad_line(c64_cycle_cnt);
                self.matrix_access(c64_cycle_cnt);
            },
            // turn off border in 38 column mode
            18 => {
                let ctrl1 = self.read_register(0xD011);
                let ctrl2 = self.read_register(0xD016);

                if (ctrl2 & 8) == 0
                {
                    if self.raster_cnt == self.dy_stop
                    {
                        self.ud_border_on = true;
                    }
                    else
                    {
                        if (ctrl1 & 0x10) != 0
                        {
                            if self.raster_cnt == self.dy_start
                            {
                                self.border_on = false;
                                self.ud_border_on = false;
                            }
                            else
                            {
                                if !self.ud_border_on
                                {
                                    self.border_on = false;
                                }
                            }
                        }
                        else
                        {
                            if !self.ud_border_on
                            {
                                self.border_on = false;
                            }
                        }
                    }
                }

                self.border_on_sample[2] = self.border_on;
                self.draw_graphics();
                self.sample_border();
                self.graphics_access();
                self.fetch_if_bad_line(c64_cycle_cnt);
                self.matrix_access(c64_cycle_cnt);
                self.last_char_data = self.char_data;
            },
            // graphics and matrix access
            19...54 => {
                self.draw_graphics();
                self.sample_border();
                self.graphics_access();
                self.fetch_if_bad_line(c64_cycle_cnt);
                self.matrix_access(c64_cycle_cnt);
                self.last_char_data = self.char_data;
            },
            // lastr graphics access, turn off matrix access,
            // turn on sprite DMA if y cooord is rightr and sprite enabled,
            // handle sprite y expansion, set BA for sprite 0
            55 => {
                self.draw_graphics();
                self.sample_border();
                self.graphics_access();
                self.display_if_bad_line();

                mask = 1;

                let spr_y_exp = self.read_register(0xD017);
                
                for _ in 0..8
                {
                    if (spr_y_exp & mask) != 0
                    {
                        self.sprite_y_exp ^= mask;
                    }
                    
                    mask <<= 1;
                }
                
                self.check_sprite_dma();

                if (self.sprite_dma_on & 0x01) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
                else
                {
                    as_mut!(self.cpu_ref).ba_low = false;
                }
            },
            // turn on border in 38 column mode, turn on sprite DMA if Y is right and sprite enabled,
            // set BA for sprite 0, display window ends here
            56 => {
                let ctrl2 = self.read_register(0xD016);

                if (ctrl2 & 8) == 0
                {
                    self.border_on = true;
                }

                self.border_on_sample[3] = self.border_on;

                self.draw_graphics();
                self.sample_border();
                self.idle_access();
                self.display_if_bad_line();
                self.check_sprite_dma();

                if (self.sprite_dma_on & 0x01) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // turn on border in 40 column mode, set BA for sprite 1, paint sprites
            57 => {
                let ctrl2 = self.read_register(0xD016);

                if (ctrl2 & 8) != 0
                {
                    self.border_on = true;
                }

                self.border_on_sample[4] = self.border_on;

                if self.sprite_draw == self.sprite_display_on
                {
                    //panic!("TODO: copy sprite draw data here");
                }

                mask = 1;

                for _ in 0..8
                {
                    if ((self.sprite_display_on & mask) != 0) && ((self.sprite_dma_on & mask) == 0)
                    {
                        self.sprite_display_on &= !mask;
                    }

                    mask <<= 1;
                }

                self.draw_background();
                self.sample_border();
                self.idle_access();
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x02) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // fetch sprite pointer 0, reset mc, turn on sprite display if needed,
            // turn off display if row_cnt == 7, read data of sprite 0
            58 => {
                self.draw_background();
                self.sample_border();

                mask = 1;

                for i in 0..8
                {
                    self.mc[i] = self.mc_base[i];

                    // TODO: fetch data from registers $D001-0F properly here
                    let my = 0;
                    if ((self.sprite_dma_on & mask) != 0) && ((self.raster_cnt & 0x00FF) == my as u16)
                    {
                        self.sprite_display_on |= mask;
                    }

                    mask <<= 1;
                }

                self.sprite_ptr_access(0);
                self.sprite_data_access(0, 0);

                if self.row_cnt == 7
                {
                    self.video_cnt_base = self.video_cnt;
                    self.display_state = false;
                }

                if self.is_bad_line || self.display_state
                {
                    self.display_state = true;
                    self.row_cnt = (self.row_cnt + 1) & 7;
                }
            },
            // set BA for sprite 2, read data of sprite 0
            59 => {
                self.draw_background();
                self.sample_border();
                self.sprite_data_access(0, 1);
                self.sprite_data_access(0, 2);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x04) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // fetch sprite pointer 1, reset BA if sprite 1 and 2 are off
            // graphics display ends here
            60 => {
                self.draw_background();
                self.sample_border();

                if self.draw_this_line
                {
                    if self.sprite_draw != 0 { self.draw_sprites(); }

                    // left border01
                    if self.border_on_sample[0]
                    {
                        for i in 0..4
                        {
                            let color_rgba = utils::fetch_c64_color_rgba(self.border_color_sample[i]);
                            utils::memset8(&mut self.window_buffer, self.line_start_offset + i*8 as usize, color_rgba);
                        }
                    }

                    // top and bottom - first 8 pixels
                    if self.border_on_sample[1]
                    {
                        let color_rgba = utils::fetch_c64_color_rgba(self.border_color_sample[4]);
                        utils::memset8(&mut self.window_buffer, self.line_start_offset + 4*8, color_rgba);
                    }

                    // top and bottom
                    if self.border_on_sample[2]
                    {
                        for i in 5..43
                        {
                            let color_rgba = utils::fetch_c64_color_rgba(self.border_color_sample[i]);
                            utils::memset8(&mut self.window_buffer, self.line_start_offset + i*8, color_rgba);
                        }
                    }

                    // top and bottom - last 8 pixels
                    if self.border_on_sample[3]
                    {
                        let color_rgba = utils::fetch_c64_color_rgba(self.border_color_sample[43]);
                        utils::memset8(&mut self.window_buffer, self.line_start_offset + 43*8, color_rgba);
                    }

                    // right border
                    if self.border_on_sample[4]
                    {
                        for i in 44..c64::SCREEN_WIDTH/8
                        {
                            let color_rgba = utils::fetch_c64_color_rgba(self.border_color_sample[i]);
                            utils::memset8(&mut self.window_buffer, self.line_start_offset + i*8, color_rgba);
                        }
                    }

                    self.line_start_offset += c64::SCREEN_WIDTH;
                }

                self.sprite_ptr_access(1);
                self.sprite_data_access(1, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x06) == 0
                {
                    as_ref!(self.cpu_ref).ba_low = false;
                }
            },
            // set BA for sprite 3, read data of sprite 1
            61 => {
                self.sprite_data_access(1, 1);
                self.sprite_data_access(1, 2);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x08) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }
            },
            // read sprite pointer 2, reset BA if sprite 2 and 3 are off, read data of sprite 2
            62 => {
                self.sprite_ptr_access(2);
                self.sprite_data_access(2, 0);
                self.display_if_bad_line();

                if (self.sprite_dma_on & 0x0C) == 0
                {
                    as_ref!(self.cpu_ref).ba_low = false;
                }
            },
            // set BA for sprite 4, read data of sprite 2
            63 => {
                self.sprite_data_access(2, 1);
                self.sprite_data_access(2, 2);
                self.display_if_bad_line();

                if self.raster_cnt == self.dy_stop
                {
                    self.ud_border_on = true;
                }
                else
                {
                    let ctrl1 = self.read_register(0xD011);

                    if ((ctrl1 & 0x10) != 0) && (self.raster_cnt == self.dy_start)
                    {
                        self.ud_border_on = false;
                    }
                }
                
                if (self.sprite_dma_on & 0x10) != 0
                {
                    self.set_ba_low(c64_cycle_cnt);
                }

                line_finished = true;
            },
            _ => (),
        }

        // next cycle
        self.raster_x = (Wrapping(self.raster_x) + Wrapping(8)).0;
        if line_finished { self.curr_cycle = 1; } else { self.curr_cycle += 1; }

        line_finished
    }
}

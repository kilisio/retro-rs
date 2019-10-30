use crate::error::*;
use libc::c_char;
use libloading::Library;
use libretro_sys::*;
use std::ffi::{c_void, CString};
use std::fs::File;
use std::io::Read;
use std::marker::PhantomData;
use std::panic;
use std::path::Path;
use std::ptr;
use crate::buttons::Buttons;

type NotSendSync = *const [u8; 0];

static mut EMULATOR: *mut EmulatorCore = ptr::null_mut();
static mut CONTEXT: *mut EmulatorContext = ptr::null_mut();

struct EmulatorCore {
    core_lib: Library,
    core_path: CString,
    rom_path: CString,
    core: CoreAPI,
    _marker: PhantomData<NotSendSync>,
}

struct EmulatorContext {
    audio_sample: Vec<i16>,
    buttons: Buttons,
    frame_ptr: *const c_void,
    frame_pitch: usize,
    frame_width: u32,
    frame_height: u32,
    pixfmt: PixelFormat,
    image_depth: usize,
    memory_map: Vec<MemoryDescriptor>,
    _marker: PhantomData<NotSendSync>,
}

// Emulator token must not be send nor sync
pub struct Emulator {
    phantom: PhantomData<NotSendSync>,
}

impl Emulator {
    pub fn create(core_path: &Path, rom_path: &Path) -> Emulator {
        unsafe {
            assert!(EMULATOR.is_null());
            assert!(CONTEXT.is_null());
        }
        let suffix = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else if cfg!(target_os = "linux") {
            "so"
        } else {
            panic!("Unsupported platform")
        };
        let dll = Library::new(core_path.with_extension(suffix)).unwrap();
        unsafe {
            let retro_set_environment = *(dll.get(b"retro_set_environment").unwrap());
            let retro_set_video_refresh = *(dll.get(b"retro_set_video_refresh").unwrap());
            let retro_set_audio_sample = *(dll.get(b"retro_set_audio_sample").unwrap());
            let retro_set_audio_sample_batch = *(dll.get(b"retro_set_audio_sample_batch").unwrap());
            let retro_set_input_poll = *(dll.get(b"retro_set_input_poll").unwrap());
            let retro_set_input_state = *(dll.get(b"retro_set_input_state").unwrap());
            let retro_init = *(dll.get(b"retro_init").unwrap());
            let retro_deinit = *(dll.get(b"retro_deinit").unwrap());
            let retro_api_version = *(dll.get(b"retro_api_version").unwrap());
            let retro_get_system_info = *(dll.get(b"retro_get_system_info").unwrap());
            let retro_get_system_av_info = *(dll.get(b"retro_get_system_av_info").unwrap());
            let retro_set_controller_port_device =
                *(dll.get(b"retro_set_controller_port_device").unwrap());
            let retro_reset = *(dll.get(b"retro_reset").unwrap());
            let retro_run = *(dll.get(b"retro_run").unwrap());
            let retro_serialize_size = *(dll.get(b"retro_serialize_size").unwrap());
            let retro_serialize = *(dll.get(b"retro_serialize").unwrap());
            let retro_unserialize = *(dll.get(b"retro_unserialize").unwrap());
            let retro_cheat_reset = *(dll.get(b"retro_cheat_reset").unwrap());
            let retro_cheat_set = *(dll.get(b"retro_cheat_set").unwrap());
            let retro_load_game = *(dll.get(b"retro_load_game").unwrap());
            let retro_load_game_special = *(dll.get(b"retro_load_game_special").unwrap());
            let retro_unload_game = *(dll.get(b"retro_unload_game").unwrap());
            let retro_get_region = *(dll.get(b"retro_get_region").unwrap());
            let retro_get_memory_data = *(dll.get(b"retro_get_memory_data").unwrap());
            let retro_get_memory_size = *(dll.get(b"retro_get_memory_size").unwrap());

            let emu = EmulatorCore {
                core_lib: dll,
                rom_path: CString::new(rom_path.to_str().unwrap()).unwrap(),
                core_path: CString::new(core_path.to_str().unwrap()).unwrap(),
                core: CoreAPI {
                    retro_set_environment,
                    retro_set_video_refresh,
                    retro_set_audio_sample,
                    retro_set_audio_sample_batch,
                    retro_set_input_poll,
                    retro_set_input_state,

                    retro_init,
                    retro_deinit,

                    retro_api_version,

                    retro_get_system_info,
                    retro_get_system_av_info,
                    retro_set_controller_port_device,

                    retro_reset,
                    retro_run,

                    retro_serialize_size,
                    retro_serialize,
                    retro_unserialize,

                    retro_cheat_reset,
                    retro_cheat_set,

                    retro_load_game,
                    retro_load_game_special,
                    retro_unload_game,

                    retro_get_region,
                    retro_get_memory_data,
                    retro_get_memory_size,
                },
                _marker: PhantomData,
            };
            let emup = Box::new(emu);
            // Store a pointer to the data
            EMULATOR = Box::leak(emup);
            // Forget the box so it doesn't drop
            let ctx = EmulatorContext {
                audio_sample: Vec::new(),
                buttons: Buttons::new(),
                frame_ptr: ptr::null(),
                frame_pitch: 0,
                frame_width: 0,
                frame_height: 0,
                pixfmt: PixelFormat::ARGB1555,
                image_depth: 0,
                memory_map: Vec::new(),
                _marker: PhantomData,
            };
            // Ditto here for the context
            let ctxp = Box::new(ctx);
            CONTEXT = Box::leak(ctxp);
            let emu = &(*EMULATOR);
            // Set up callbacks
            (emu.core.retro_set_environment)(callback_environment);
            (emu.core.retro_set_video_refresh)(callback_video_refresh);
            (emu.core.retro_set_audio_sample)(callback_audio_sample);
            (emu.core.retro_set_audio_sample_batch)(callback_audio_sample_batch);
            (emu.core.retro_set_input_poll)(callback_input_poll);
            (emu.core.retro_set_input_state)(callback_input_state);
            // Load the game
            (emu.core.retro_init)();
            let mut sys_info = SystemInfo {
                library_name: ptr::null(),
                library_version: ptr::null(),
                valid_extensions: ptr::null(),
                need_fullpath: false,
                block_extract: false,
            };
            retro_get_system_info(&mut sys_info);
            let rom_cstr = &(*EMULATOR).rom_path;

            let mut rom_file = File::open(rom_path).unwrap();
            let mut buffer = Vec::new();
            rom_file.read_to_end(&mut buffer).unwrap();
            buffer.shrink_to_fit();
            let game_info = GameInfo {
                path: rom_cstr.as_ptr(),
                data: buffer.as_ptr() as *const c_void,
                size: buffer.len(),
                meta: ptr::null(),
            };
            (emu.core.retro_load_game)(&game_info);
            let mut av_info = SystemAvInfo {
                geometry: GameGeometry {
                    base_width: 0,
                    base_height: 0,
                    max_width: 0,
                    max_height: 0,
                    aspect_ratio: 0.0,
                },
                timing: SystemTiming {
                    fps: 0.0,
                    sample_rate: 0.0,
                },
            };
            (retro_get_system_av_info)(&mut av_info);
            Emulator {
                phantom: PhantomData,
            }
        }
    }
    pub fn run(&mut self, inputs: Buttons) {
        unsafe {
            //clear audio buffers and whatever else
            (*CONTEXT).audio_sample.clear();
            //set inputs on CB
            (*CONTEXT).buttons = inputs;
            //run one step
            ((*EMULATOR).core.retro_run)()
        }
    }
    pub fn reset(&mut self) {
        unsafe {
            //clear audio buffers and whatever else
            (*CONTEXT).audio_sample.clear();
            //clear inputs on CB
            (*CONTEXT).buttons = Buttons::new();
            //clear fb
            (*CONTEXT).frame_ptr = ptr::null();
            ((*EMULATOR).core.retro_reset)()
        }
    }
    pub fn pixel_format(&self) -> PixelFormat {
        unsafe { (*CONTEXT).pixfmt }
    }
    pub fn framebuffer_size(&self) -> (usize, usize) {
        unsafe {
            (
                (*CONTEXT).frame_width as usize,
                (*CONTEXT).frame_height as usize,
            )
        }
    }
    pub fn framebuffer_pitch(&self) -> usize {
        unsafe { (*CONTEXT).frame_pitch }
    }
    pub fn peek_framebuffer<FBPeek, FBPeekRet>(&self, f: FBPeek) -> FBPeekRet
    where
        FBPeek: FnOnce(Option<&[u8]>) -> FBPeekRet,
    {
        unsafe {
            if (*CONTEXT).frame_ptr.is_null() {
                f(None)
            } else {
                let frame_slice = std::slice::from_raw_parts(
                    (*CONTEXT).frame_ptr as *const u8,
                    ((*CONTEXT).frame_width
                        * (*CONTEXT).frame_height
                        * ((*CONTEXT).frame_pitch as u32)) as usize,
                );
                f(Some(frame_slice))
            }
        }
    }
    pub fn save(&self, bytes: &mut [u8]) {
        let size = self.save_size();
        assert!(bytes.len() >= size);
        unsafe { ((*EMULATOR).core.retro_serialize)(bytes.as_mut_ptr() as *mut c_void, size) }
    }
    pub fn load(&mut self, bytes: &[u8]) -> bool {
        let size = self.save_size();
        assert!(bytes.len() >= size);
        unsafe { ((*EMULATOR).core.retro_unserialize)(bytes.as_ptr() as *const c_void, size) }
    }
    pub fn save_size(&self) -> usize {
        unsafe { ((*EMULATOR).core.retro_serialize_size)() }
    }
    pub fn clear_cheats(&mut self) {
        unsafe { ((*EMULATOR).core.retro_cheat_reset)() }
    }
    pub fn set_cheat(&mut self, index: usize, enabled: bool, code: &str) {
        unsafe {
            // FIXME: Creates a memory leak since the libretro api won't let me from_raw() it back and drop it.  I don't know if libretro guarantees anything about ownership of this str to cores.
            ((*EMULATOR).core.retro_cheat_set)(
                index as u32,
                enabled,
                CString::new(code).unwrap().into_raw(),
            )
        }
    }
    pub fn copy_framebuffer(&self, slice: &mut [u8]) -> Result<(), RetroRsError> {
        let (w, h) = self.framebuffer_size();
        let fmt = self.pixel_format();
        self.peek_framebuffer(|fb| {
            let fb = fb.ok_or(RetroRsError::NoFramebufferError)?;
            match fmt {
                PixelFormat::ARGB1555 => {
                    for y in 0..h {
                        for x in 0..w {
                            let start = y * w + x;
                            let gb = fb[start * 2];
                            let arg = fb[start * 2 + 1];
                            let (red, green, blue) = argb555to888(gb, arg);

                            slice[start * 3] = red;
                            slice[start * 3 + 1] = green;
                            slice[start * 3 + 2] = blue;
                        }
                    }
                }
                PixelFormat::ARGB8888 => {
                    for y in 0..h {
                        for x in 0..w {
                            let off = (y * w + x) * 4;
                            slice[off] = fb[off + 1];
                            slice[off + 1] = fb[off + 2];
                            slice[off + 2] = fb[off + 3];
                        }
                    }
                }
                PixelFormat::RGB565 => {
                    for y in 0..h {
                        for x in 0..w {
                            let start = y * w + x;
                            let gb = fb[start * 2];
                            let rg = fb[start * 2 + 1];
                            let (red, green, blue) = rgb565to888(gb, rg);
                            slice[start * 3] = red;
                            slice[start * 3 + 1] = green;
                            slice[start * 3 + 2] = blue;
                        }
                    }
                }
            };
            Result::Ok(())
        })
    }
}

unsafe extern "C" fn callback_environment(cmd: u32, data: *mut c_void) -> bool {
    let result = panic::catch_unwind(|| {
        match cmd {
            ENVIRONMENT_SET_PIXEL_FORMAT => {
                let pixfmti = *(data as *const u32);
                let pixfmt = PixelFormat::from_uint(pixfmti);
                if pixfmt.is_none() {
                    return false;
                }
                let pixfmt = pixfmt.unwrap();
                (*CONTEXT).image_depth = match pixfmt {
                    PixelFormat::ARGB1555 => 15,
                    PixelFormat::ARGB8888 => 32,
                    PixelFormat::RGB565 => 16,
                };
                (*CONTEXT).pixfmt = pixfmt;
                true
            }
            ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
                *(data as *mut *const c_char) = (*EMULATOR).core_path.as_ptr();
                true
            }
            ENVIRONMENT_GET_CAN_DUPE => {
                *(data as *mut bool) = true;
                true
            }
            ENVIRONMENT_SET_MEMORY_MAPS => {
                let map = data as *const MemoryMap;
                let desc_slice =
                    std::slice::from_raw_parts((*map).descriptors, (*map).num_descriptors as usize);
                // Don't know who owns map or how long it will last
                (*CONTEXT).memory_map = Vec::new();
                // So we had better copy it
                (*CONTEXT).memory_map.extend_from_slice(desc_slice);
                // (Implicitly we also want to drop the old one, which we did by reassigning)
                true
            }
            _ => false,
        }
    });
    result.unwrap_or(false)
}

extern "C" fn callback_video_refresh(data: *const c_void, width: u32, height: u32, pitch: usize) {
    // Can't panic
    unsafe {
        // context's framebuffer just points to the given data.  Seems to work OK for gym-retro.
        if !data.is_null() {
            (*CONTEXT).frame_ptr = data;
            (*CONTEXT).frame_pitch = pitch;
            (*CONTEXT).frame_width = width;
            (*CONTEXT).frame_height = height;
        }
    }
}
extern "C" fn callback_audio_sample(left: i16, right: i16) {
    // Can't panic
    unsafe {
        let sample_buf = &mut (*CONTEXT).audio_sample;
        sample_buf.push(left);
        sample_buf.push(right);
    }
}
extern "C" fn callback_audio_sample_batch(data: *const i16, frames: usize) -> usize {
    // Can't panic
    unsafe {
        let sample_buf = &mut (*CONTEXT).audio_sample;
        let slice = std::slice::from_raw_parts(data, frames * 2);
        sample_buf.clear();
        sample_buf.extend_from_slice(slice);
        frames
    }
}

extern "C" fn callback_input_poll() {}

extern "C" fn callback_input_state(port: u32, device: u32, index: u32, id: u32) -> i16 {
    // Can't panic
    if port != 0 || device != 0 || index != 0 {
        // Unsupported port/device/index
        return 0;
    }
    unsafe {
        let id = id as usize;
        if id > 16 {
            print!("Unexpected button id {}", id);
            return 0;
        }
        (*CONTEXT).buttons.get(id)
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        unsafe {
            ((*EMULATOR).core.retro_unload_game)();
            ((*EMULATOR).core.retro_deinit)();
        }
        //TODO drop memory maps etc
        unsafe {
            // "remember" context and emulator we forgot before
            let _ctx = Box::from_raw(CONTEXT);
            let _emu = Box::from_raw(EMULATOR);
            CONTEXT = ptr::null_mut();
            EMULATOR = ptr::null_mut();
        }
        // let them drop naturally
    }
}

pub fn argb555to888(lo: u8, hi: u8) -> (u8, u8, u8) {
    let r = (hi & 0b0111_1100) >> 2;
    let g = ((hi & 0b0000_0011) << 3) + ((lo & 0b1110_0000) >> 5);
    let b = lo & 0b0001_1111;
    // Use high bits for empty low bits
    let r = (r << 3) | (r >> 2);
    let g = (g << 3) | (g >> 2);
    let b = (b << 3) | (b >> 2);
    (r, g, b)
}

pub fn rgb565to888(lo: u8, hi: u8) -> (u8, u8, u8) {
    let r = (hi & 0b1111_1000) >> 3;
    let g = ((hi & 0b0000_0111) << 3) + ((lo & 0b1110_0000) >> 5);
    let b = lo & 0b0001_1111;
    // Use high bits for empty low bits
    let r = (r << 3) | (r >> 2);
    let g = (g << 2) | (g >> 3);
    let b = (b << 3) | (b >> 2);
    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[cfg(feature = "use_image")]
    extern crate image;
    #[cfg(feature = "use_image")]
    use crate::fb_to_image::*;

    #[cfg(feature = "use_image")]
    #[test]
    fn it_works() {
        let mut emu = Emulator::create(
            Path::new("cores/fceumm_libretro"),
            Path::new("roms/mario.nes"),
        );
        for _ in 0..100 {
            emu.run(Buttons::new());
        }
        let fb = emu.create_imagebuffer();
        fb.unwrap().save("out.png").unwrap();
        emu.reset();
        for _ in 0..100 {
            emu.run(Buttons::new());
        }
        //emu will drop naturally
    }
}

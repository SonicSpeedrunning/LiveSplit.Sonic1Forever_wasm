#![no_std]
use asr::{signature::Signature, timer, timer::TimerState, watcher::Watcher, Address, Process, time::Duration};

#[cfg(all(not(test), target_arch = "wasm32"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

static AUTOSPLITTER: spinning_top::Spinlock<State> = spinning_top::const_spinlock(State {
    game: None,
    watchers: Watchers {
        state: Watcher::new(),
        levelid: Watcher::new(),
        zoneselectongamecomplete: Watcher::new(),
        zoneindicator: Watcher::new(),
    },
    settings: None,
});

struct State {
    game: Option<ProcessInfo>,
    watchers: Watchers,
    settings: Option<Settings>,
}

struct ProcessInfo {
    game: Process,
    is_64_bit: bool,
    game_version: GameVersion,
    main_module_base: Address,
    main_module_size: u64,
    addresses: Option<MemoryPtr>,
}

struct Watchers {
    state: Watcher<u8>,
    levelid: Watcher<Acts>,
    zoneselectongamecomplete: Watcher<u8>,
    zoneindicator: Watcher<ZoneIndicator>,
}

struct MemoryPtr {
    state: Address,
    levelid: Address,
    zoneselectongamecomplete: Address,
    zoneindicator: Address,
}

#[derive(asr::Settings)]
struct Settings {
    #[default = true]
    /// Start --> New Game
    start_clean_save: bool,
    #[default = true]
    /// Start --> New Game+
    start_new_game_plus: bool,
    #[default = true]
    /// Reset --> Enable auto reset
    reset: bool,
    #[default = true]
    /// Green Hill Zone - Act 1
    green_hill_1: bool,
    #[default = true]
    /// Green Hill Zone - Act 2
    green_hill_2: bool,
    #[default = true]
    /// Green Hill Zone - Act 3
    green_hill_3: bool,
    #[default = true]
    /// Marble Zone - Act 1
    marble_1: bool,
    #[default = true]
    /// Marble Zone - Act 2
    marble_2: bool,
    #[default = true]
    /// Marble Zone - Act 3
    marble_3: bool,
    #[default = true]
    /// Spring Yard Zone - Act 1
    spring_yard_1: bool,
    #[default = true]
    /// Spring Yard Zone - Act 2
    spring_yard_2: bool,
    #[default = true]
    /// Spring Yard Zone - Act 3
    spring_yard_3: bool,
    #[default = true]
    /// Labyrinth Zone - Act 1
    labyrinth_1: bool,
    #[default = true]
    /// Labyrinth Zone - Act 2
    labyrinth_2: bool,
    #[default = true]
    /// Labyrinth Zone - Act 3
    labyrinth_3: bool,
    #[default = true]
    /// Star Light Zone - Act 1
    star_light_1: bool,
    #[default = true]
    /// Star Light Zone - Act 2
    star_light_2: bool,
    #[default = true]
    /// Star Light Zone - Act 3
    star_light_3: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 1
    scrap_brain_1: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 2
    scrap_brain_2: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 3
    scrap_brain_3: bool,
    #[default = true]
    /// Final Zone
    final_zone: bool,
}

impl ProcessInfo {
    pub fn attach_process() -> Option<Self> {
        const PROCESS_NAMES: [&str; 1] = ["SonicForever.exe"];
        let mut proc: Option<Process> = None;
        let mut proc_name: Option<&str> = None;
    
        for name in PROCESS_NAMES {
            proc = Process::attach(name);
            if proc.is_some() {
                proc_name = Some(name);
                break;
            }
        }
    
        let game = proc?;
        let main_module_base = game.get_module_address(proc_name?).ok()?;
        let main_module_size: u64 = game.get_module_size(proc_name?).ok()?;

        // Determine game version through hacky ways
        const SIG64: Signature<15> = Signature::new("81 F9 ???????? 0F 87 ???????? 41 8B 8C");
        const SIG32: Signature<19> = Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
        
        let is_64_bit = if let Some(_is64bit) = SIG64.scan_process_range(&game, main_module_base, main_module_size) {
            true
        } else if let Some(_is32bit) = SIG32.scan_process_range(&game, main_module_base, main_module_size) {
            false
        } else {
            return None
        };

        let game_version = if is_64_bit {
            GameVersion::Below1_5_0
        } else {
            if main_module_size < 0x57F4000 as u64 {
                GameVersion::Below1_5_0
            } else {
                GameVersion::V1_5_0OrHigher
            }
        };
   
        Some(Self {
            game,
            is_64_bit,
            game_version,
            main_module_base,
            main_module_size,
            addresses: None,
        })
    }
}

impl State {
    fn init(&mut self) -> bool {        
        if self.game.is_none() {
            self.game = ProcessInfo::attach_process()
        }

        let Some(game) = &mut self.game else {
            return false
        };

        if !game.game.is_open() {
            self.game = None;
            return false
        }

        if game.addresses.is_none() {
            game.addresses = MemoryPtr::new(&game.game, game.main_module_base, game.main_module_size, game.is_64_bit, game.game_version)
        }

        game.addresses.is_some()   
    }

    fn update(&mut self) {
        let Some(game) = &self.game else { return };
        let Some(addresses) = &game.addresses else { return };
        let proc = &game.game;

        let Some(_thing) = self.watchers.state.update(proc.read(addresses.state).ok()) else { return };
        let Some(_thing) = self.watchers.zoneselectongamecomplete.update(if game.game_version == GameVersion::V1_5_0OrHigher { proc.read(addresses.zoneselectongamecomplete).ok() } else  { Some(0) }) else { return };

        let Some(g) = proc.read::<u32>(addresses.zoneindicator).ok() else { return };
        let i: ZoneIndicator = match &g {
            0x6E69614D => ZoneIndicator::MainMenu,
            0x656E6F5A => ZoneIndicator::Zones,
            0x69646E45 => ZoneIndicator::Ending,
            0x65766153 => ZoneIndicator::SaveSelect,
            _ => ZoneIndicator::Default
        };
        let Some(zone) = self.watchers.zoneindicator.update(Some(i)) else { return };

        if zone.current == ZoneIndicator::Ending {
            self.watchers.levelid.update(Some(Acts::Default));
        } else if zone.current == ZoneIndicator::Zones {
            let Some(g) = proc.read::<u8>(addresses.levelid).ok() else { return };
            let i: Acts = match g {
                0 => Acts::GreenHill1,
                1 => Acts::GreenHill2,
                2 => Acts::GreenHill3,
                3 => Acts::Marble1,
                4 => Acts::Marble2,
                5 => Acts::Marble3,
                6 => Acts::SpringYard1,
                7 => Acts::SpringYard2,
                8 => Acts::SpringYard3,
                9 => Acts::Labyrinth1,
                10 => Acts::Labyrinth2,
                11 => Acts::Labyrinth3,
                12 => Acts::StarLight1,
                13 => Acts::StarLight2,
                14 => Acts::StarLight3,
                15 => Acts::ScrapBrain1,
                16 => Acts::ScrapBrain2,
                17 => Acts::ScrapBrain3,
                18 => Acts::FinalZone,
                _ => Acts::Default,
            };
            self.watchers.levelid.update(Some(i));
        } else {
            let Some(x) = &self.watchers.levelid.pair else { return };
            let x = x.current;
            self.watchers.levelid.update(Some(x));
        }
    }

    fn start(&mut self) -> bool {
        let Some(game) = &self.game else { return false };
        let Some(state) = &self.watchers.state.pair else { return false };
        let Some(zoneselectongamecomplete) = &self.watchers.zoneselectongamecomplete.pair else { return false };
        let Some(zoneindicator) = &self.watchers.zoneindicator.pair else { return false };
        let Some(settings) = &self.settings else { return false };

        if game.game_version == GameVersion::Below1_5_0 {
            let run_started_save_file = state.changed() && state.current == 2 && zoneindicator.current == ZoneIndicator::SaveSelect;
            let run_started_no_save_file = state.old == 6 && state.current == 7;
            let run_started_ngp = state.old == 8 && state.current == 9;

            (settings.start_clean_save && (run_started_save_file || run_started_no_save_file)) || (settings.start_new_game_plus && run_started_ngp) 
        } else {
            let run_started_save_file = state.old == 3 && state.current == 7 && zoneindicator.current == ZoneIndicator::SaveSelect;
            let run_started_no_save_file = state.old == 10 && state.current == 11;
            let run_started_ngp = state.old == 2 && state.current == 6 && zoneselectongamecomplete.current == 0;

            (settings.start_clean_save && (run_started_save_file || run_started_no_save_file)) || (settings.start_new_game_plus && run_started_ngp) 
        }
    }

    fn split(&mut self) -> bool {
        let Some(levelid) = &self.watchers.levelid.pair else { return false };
        let Some(settings) = &self.settings else { return false };
    
        match levelid.current {
            Acts::GreenHill2 => settings.green_hill_1 && levelid.old == Acts::GreenHill1,
            Acts::GreenHill3 => settings.green_hill_2 && levelid.old == Acts::GreenHill2,
            Acts::Marble1 => settings.green_hill_3 && levelid.old == Acts::GreenHill3,
            Acts::Marble2 => settings.marble_1 && levelid.old == Acts::Marble1,
            Acts::Marble3 => settings.marble_2 && levelid.old == Acts::Marble2,
            Acts::SpringYard1 => settings.marble_3 && levelid.old == Acts::Marble3,
            Acts::SpringYard2 => settings.spring_yard_1 && levelid.old == Acts::SpringYard1,
            Acts::SpringYard3 => settings.spring_yard_2 && levelid.old == Acts::SpringYard2,
            Acts::Labyrinth1 => settings.spring_yard_3 && levelid.old == Acts::SpringYard3,
            Acts::Labyrinth2 => settings.labyrinth_1 && levelid.old == Acts::Labyrinth1,
            Acts::Labyrinth3 => settings.labyrinth_2 && levelid.old == Acts::Labyrinth2,
            Acts::StarLight1 => settings.labyrinth_3 && levelid.old == Acts::Labyrinth3,
            Acts::StarLight2 => settings.star_light_1 && levelid.old == Acts::StarLight1,
            Acts::StarLight3 => settings.star_light_2 && levelid.old == Acts::StarLight2,
            Acts::ScrapBrain1 => settings.star_light_3 && levelid.old == Acts::StarLight3,
            Acts::ScrapBrain2 => settings.scrap_brain_1 && levelid.old == Acts::ScrapBrain1,
            Acts::ScrapBrain3 => settings.scrap_brain_2 && levelid.old == Acts::ScrapBrain2,
            Acts::FinalZone => settings.scrap_brain_3 && levelid.old == Acts::ScrapBrain3,
            Acts::Default => settings.final_zone && levelid.old != levelid.current,
            _ => false,
        }
    }

    fn reset(&mut self) -> bool {
        let Some(settings) = &self.settings else { return false };
        
        if !settings.reset {
            return false
        }

        let Some(state) = &self.watchers.state.pair else { return false };
        let Some(zoneindicator) = &self.watchers.zoneindicator.pair else { return false };
        let Some(game) = &self.game else { return false };

        if game.game_version == GameVersion::Below1_5_0 {
            state.current == 201 && state.old == 200 && zoneindicator.current == ZoneIndicator::SaveSelect
        } else {
            state.current == 14 && state.old == 13 && zoneindicator.current == ZoneIndicator::SaveSelect
        }
    }

    fn is_loading(&mut self) -> Option<bool> {
        None
    }

    fn game_time(&mut self) -> Option<Duration> {
        None
    }
}

impl MemoryPtr {
    fn new(process: &Process, addr: Address, size: u64, is_64_bit: bool, version: GameVersion) -> Option<Self> {
        fn pointerpath(process: &Process, base_address: Address, ptr: Address, lea: Address, offset1: u32, offset2: u32, offset3: u32, absolute: bool, is_64_bit: bool) -> Option<Address> {
            if is_64_bit {
                if offset1 == 0 {
                    return Some(lea + offset3)
                }

                let temp_offset = process.read::<u32>(ptr + offset1).ok()?;
                let temp_offset_2 = base_address + temp_offset + offset2;

                if absolute {
                    Some(base_address + process.read::<u32>(temp_offset_2).ok()? + offset3)
                } else {
                    Some(temp_offset_2 + 0x4 as u32 + process.read::<u32>(temp_offset_2).ok()? + offset3)
                }
            } else {
                let result = process.read_pointer_path32::<u32>(ptr.0 as u32, &[offset1, offset2]).ok()? as u64;
                Some(Address(result + offset3 as u64))
            }
        }

        if version == GameVersion::Below1_5_0 {
            if is_64_bit {
                const SIG_64: Signature<15> = Signature::new("81 F9 ???????? 0F 87 ???????? 41 8B 8C");
                let ptr = SIG_64.scan_process_range(process, addr, size)?.0 + 16;
                let ptr = Address(addr.0 + process.read::<u32>(Address(ptr)).ok()? as u64);

                const SIG_64_2: Signature<11> = Signature::new("48 8D 05 ???????? 49 63 F8 4C");
                let lea = SIG_64_2.scan_process_range(process, addr, size)?.0 + 3;
                let lea = Address(lea + 0x4 + process.read::<u32>(Address(lea)).ok()? as u64);

                let state = pointerpath(process, addr, ptr, lea, 0x4 * 0, 0, 0x9EC, false, is_64_bit)?;
                let levelid = pointerpath(process, addr, ptr, lea, 0x4 * 123, 2, 0, false, is_64_bit)?;
                let zoneselectongamecomplete = Address(0);

                const SIG_64_3: Signature<15> = Signature::new("C6 05 ???????? ?? E9 ???????? 48 8D 0D");
                let ptr = SIG_64_3.scan_process_range(process, addr, size)?.0 + 2;
                let zoneindicator = Address(ptr + 0x5 + process.read::<u32>(Address(ptr)).ok()? as u64);

                Some(Self {
                    state,
                    levelid,
                    zoneselectongamecomplete,
                    zoneindicator,
                })
            } else {
                const SIG32: Signature<19> = Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
                let ptr = SIG32.scan_process_range(process, addr, size)?.0 + 14;
                let ptr = Address(process.read::<u32>(Address(ptr)).ok()? as u64);
                
                let lea = Address(0);

                let state = pointerpath(process, addr, ptr, lea, 0x4 * 73, 8, 0x9D8, true, is_64_bit)?;
                let levelid = pointerpath(process, addr, ptr, lea, 0x4 * 123, 1, 0, true, is_64_bit)?;
                let zoneselectongamecomplete = Address(0);

                const SIG32_2: Signature<7> = Signature::new("69 F8 ???????? B8");
                let ptr = SIG32_2.scan_process_range(process, addr, size)?.0 + 7;
                let zoneindicator = Address(process.read::<u32>(Address(ptr)).ok()? as u64);

                Some(Self {
                    state,
                    levelid,
                    zoneselectongamecomplete,
                    zoneindicator,
                })
            }
        } else {
            if is_64_bit {
                None
            } else {
                const SIG32: Signature<19> = Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
                let ptr = SIG32.scan_process_range(process, addr, size)?.0 + 14;
                let ptr = Address(process.read::<u32>(Address(ptr)).ok()? as u64);
                let lea = Address(0);

                let state = pointerpath(process, addr, ptr, lea, 0x4 * 30, 8, 0x9D8, true, is_64_bit)?;
                let levelid = pointerpath(process, addr, ptr, lea, 0x4 * 123, 1, 0, true, is_64_bit)?;
                let zoneselectongamecomplete = pointerpath(process, addr, ptr, lea, 0x4 * 18, 3, 4, true, is_64_bit)?;

                const SIG32_2: Signature<7> = Signature::new("69 F8 ???????? B8");
                let ptr = SIG32_2.scan_process_range(process, addr, size)?.0 + 7;
                let zoneindicator = Address(process.read::<u32>(Address(ptr)).ok()? as u64);

                Some(Self {
                    state,
                    levelid,
                    zoneselectongamecomplete,
                    zoneindicator,
                })
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn update() {
    // Get access to the spinlock
    let autosplitter = &mut AUTOSPLITTER.lock();
    
    // Sets up the settings
    autosplitter.settings.get_or_insert_with(Settings::register);

    // Main autosplitter logic, essentially refactored from the OG LivaSplit autosplitting component.
    // First of all, the autosplitter needs to check if we managed to attach to the target process,
    // otherwise there's no need to proceed further.
    if !autosplitter.init() {
        return
    }

    // The main update logic is launched with this
    autosplitter.update();

    let timer_state = timer::state();

    // Splitting logic. Adapted from OG LiveSplit:
    // Order of execution
    // 1. update() [this is launched above] will always be run first. There are no conditions on the execution of this action.
    // 2. If the timer is currently either running or paused, then the isLoading, gameTime, and reset actions will be run.
    // 3. If reset does not return true, then the split action will be run.
    // 4. If the timer is currently not running (and not paused), then the start action will be run.
    if timer_state == TimerState::Running || timer_state == TimerState::Paused {
        if let Some(is_loading) = autosplitter.is_loading() {
            if is_loading {
                timer::pause_game_time()
            } else {
                timer::resume_game_time()
            }
        }

        if let Some(game_time) = autosplitter.game_time() {
            timer::set_game_time(game_time)
        }

        if autosplitter.reset() {
            timer::reset()
        } else if autosplitter.split() {
            timer::split()
        }
    } 

    if timer_state == TimerState::NotRunning {
        if autosplitter.start() {
            timer::start()
        }
    }     
}


#[derive(Clone, Copy, PartialEq)]
enum ZoneIndicator {
    MainMenu,
    Zones,
    Ending,
    SaveSelect,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
enum Acts {
    GreenHill1,
    GreenHill2,
    GreenHill3,
    Marble1,
    Marble2,
    Marble3,
    SpringYard1,
    SpringYard2,
    SpringYard3,
    Labyrinth1,
    Labyrinth2,
    Labyrinth3,
    StarLight1,
    StarLight2,
    StarLight3,
    ScrapBrain1,
    ScrapBrain2,
    ScrapBrain3,
    FinalZone,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
enum GameVersion {
    Below1_5_0,
    V1_5_0OrHigher,
}

/*
    enum GameVersionSize {
        v_1_5_0_32 = 0x57F4000,
        v_1_3_4_32 = 0x3623000,
        v_1_2_1_32 = 0x362B000,
    }
*/